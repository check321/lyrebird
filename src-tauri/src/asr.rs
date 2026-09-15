//! ASR sidecar：调用 Python worker 跑 MOSS-Transcribe-Diarize，结果写入 cues。

use crate::ingest;
use serde::Deserialize;
use sqlx::SqlitePool;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Deserialize)]
struct WorkerOut {
    segments: Option<Vec<Segment>>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Segment {
    start: f64,
    end: f64,
    speaker: Option<String>,
    text: String,
}

#[derive(Debug, Clone, serde::Serialize)]
struct AsrProgress {
    video_id: i64,
    stage: String,
}

async fn setting(pool: &SqlitePool, key: &str) -> Option<String> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()?;
    row.map(|r| r.0).filter(|s| !s.trim().is_empty())
}

/// 对无字幕视频执行本地 ASR 转写，生成 cues。
#[tauri::command]
pub async fn transcribe_video(
    pool: tauri::State<'_, SqlitePool>,
    media_dir: tauri::State<'_, PathBuf>,
    app: AppHandle,
    video_id: i64,
) -> Result<usize, String> {
    let python = setting(&pool, "asr_python_path")
        .await
        .unwrap_or_else(|| "python3".into());
    let model = setting(&pool, "asr_model_path")
        .await
        .ok_or("请先在设置页配置 ASR 模型权重目录")?;

    let (video_path, audio_path, yt_id): (Option<String>, Option<String>, Option<String>) =
        sqlx::query_as("SELECT video_path, audio_path, youtube_id FROM videos WHERE id = ?")
            .bind(video_id)
            .fetch_optional(pool.inner())
            .await
            .map_err(|e| e.to_string())?
            .ok_or("视频不存在")?;

    // 确保音轨存在
    let audio = match audio_path {
        Some(p) if PathBuf::from(&p).exists() => PathBuf::from(p),
        _ => {
            let vp = video_path.ok_or("视频文件缺失")?;
            let wav = media_dir.join(format!(
                "{}.wav",
                yt_id.as_deref().unwrap_or("audio")
            ));
            let _ = app.emit(
                "asr-progress",
                AsrProgress {
                    video_id,
                    stage: "提取音轨…".into(),
                },
            );
            let ffmpeg = setting(&pool, "ffmpeg_path").await;
            ingest::extract_audio(&PathBuf::from(vp), &wav, ffmpeg.as_deref())
                .await
                .map_err(|e| e.to_string())?;
            sqlx::query("UPDATE videos SET audio_path = ? WHERE id = ?")
                .bind(wav.to_string_lossy().to_string())
                .bind(video_id)
                .execute(pool.inner())
                .await
                .map_err(|e| e.to_string())?;
            wav
        }
    };

    let worker = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("无法定位项目目录")?
        .join("python/asr_worker.py");

    let _ = app.emit(
        "asr-progress",
        AsrProgress {
            video_id,
            stage: "模型转写中（长音频可能需要几分钟）…".into(),
        },
    );

    let output = tokio::process::Command::new(python)
        .arg(worker)
        .arg("--model")
        .arg(model)
        .arg("--audio")
        .arg(&audio)
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "找不到 Python 解释器，请在设置页配置正确的路径".to_string()
            } else {
                e.to_string()
            }
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: String = stderr.lines().rev().take(5).collect::<Vec<_>>().join("\n");
        return Err(format!("ASR 进程失败:\n{tail}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // worker 可能先打印日志，JSON 结果在最后一行
    let json_line = stdout
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with('{'))
        .ok_or("ASR 输出为空")?;
    let parsed: WorkerOut = serde_json::from_str(json_line)
        .map_err(|e| format!("ASR 输出解析失败: {e}"))?;
    if let Some(err) = parsed.error {
        return Err(err);
    }
    let segments = parsed.segments.unwrap_or_default();
    if segments.is_empty() {
        return Err("未转写出任何内容".into());
    }

    // 覆盖旧 cues（ASR 重跑场景）
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM cues WHERE video_id = ?")
        .bind(video_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    for (idx, s) in segments.iter().enumerate() {
        sqlx::query(
            "INSERT INTO cues (video_id, idx, start_secs, end_secs, text_en, speaker) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(video_id)
        .bind(idx as i64)
        .bind(s.start)
        .bind(s.end)
        .bind(&s.text)
        .bind(&s.speaker)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }
    sqlx::query("UPDATE videos SET subtitle_source = 'asr' WHERE id = ?")
        .bind(video_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;

    let _ = app.emit(
        "asr-progress",
        AsrProgress {
            video_id,
            stage: "完成".into(),
        },
    );
    Ok(segments.len())
}

/// 检查 ASR Python 环境是否可用（依赖是否装好）。
#[tauri::command]
pub async fn check_asr_env(pool: tauri::State<'_, SqlitePool>) -> Result<String, String> {
    let python = setting(&pool, "asr_python_path")
        .await
        .unwrap_or_else(|| "python3".into());
    let output = tokio::process::Command::new(&python)
        .args(["-c", "import torch, transformers, moss_transcribe_diarize; print('ok')"])
        .output()
        .await
        .map_err(|e| format!("无法运行 {python}: {e}"))?;
    if output.status.success() {
        Ok(format!("ASR 环境正常（{python}）"))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: String = stderr.lines().last().unwrap_or("").to_string();
        Err(format!(
            "ASR 环境未就绪: {tail}\n请运行 scripts/setup_asr.sh 安装，并在设置页配置 Python 解释器路径"
        ))
    }
}
