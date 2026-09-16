//! 视频导出：ffmpeg 剪辑 + ASS 字幕烧录、胶片条缩略图、导出字体同步、Finder 定位。

use serde::Serialize;
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// 当前导出子进程，供 cancel_export 终止；try_wait 轮询而不长持锁，避免 cancel 拿不到锁
#[derive(Default)]
pub struct ExportState(pub tokio::sync::Mutex<Option<Arc<tokio::sync::Mutex<tokio::process::Child>>>>);

#[derive(Debug, Clone, Serialize)]
struct ExportProgress {
    video_id: i64,
    percent: f64,
}

async fn setting(pool: &SqlitePool, key: &str) -> Option<String> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()?;
    row.map(|r| r.0).filter(|s| !s.trim().is_empty())
}

async fn ffmpeg_bin(pool: &SqlitePool) -> String {
    setting(pool, "ffmpeg_path")
        .await
        .unwrap_or_else(|| "ffmpeg".into())
}

async fn video_path(pool: &SqlitePool, video_id: i64) -> Result<PathBuf, String> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT video_path FROM videos WHERE id = ?")
            .bind(video_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    row.and_then(|r| r.0)
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .ok_or_else(|| "视频文件不存在".to_string())
}

/// ffmpeg 滤镜参数转义：单引号包裹；`\` 双写防二次解转义，`'` 用 '\'' 闭合重开模式
fn esc_filter_path(p: &str) -> String {
    let p = p.replace('\\', "\\\\").replace('\'', "'\\''");
    format!("'{p}'")
}

/// 一条字幕叠层：全帧透明 PNG + 显示时间窗（秒，相对导出片起点）
#[derive(Debug, serde::Deserialize)]
pub struct OverlayPng {
    pub start: f64,
    pub end: f64,
    pub png: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub struct FfmpegFeatures {
    /// 是否编译了 libass（ass/subtitles 滤镜）；精简版 ffmpeg 没有
    pub ass: bool,
}

#[tauri::command]
pub async fn ffmpeg_features(
    pool: tauri::State<'_, SqlitePool>,
) -> Result<FfmpegFeatures, String> {
    let bin = ffmpeg_bin(&pool).await;
    let out = Command::new(&bin)
        .args(["-hide_banner", "-filters"])
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "找不到 ffmpeg，请在设置页配置路径".to_string()
            } else {
                e.to_string()
            }
        })?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let ass = stdout.lines().any(|l| {
        let mut parts = l.split_whitespace();
        matches!(parts.nth(1), Some("ass") | Some("subtitles"))
    });
    Ok(FfmpegFeatures { ass })
}

#[tauri::command]
pub async fn export_video(
    app: AppHandle,
    pool: tauri::State<'_, SqlitePool>,
    state: tauri::State<'_, ExportState>,
    media_dir: tauri::State<'_, PathBuf>,
    video_id: i64,
    out_path: String,
    start_secs: Option<f64>,
    end_secs: Option<f64>,
    ass_content: Option<String>,
    srt_content: Option<String>,
    overlays: Vec<OverlayPng>,
    total_secs: f64,
) -> Result<(), String> {
    let video = video_path(&pool, video_id).await?;
    let ffmpeg = ffmpeg_bin(&pool).await;

    let mut args: Vec<String> = Vec::new();
    // 输入侧 seek：配合重编码做到帧级精确的剪辑点
    if let (Some(s), Some(e)) = (start_secs, end_secs) {
        if e > s {
            args.push("-ss".into());
            args.push(format!("{s:.3}"));
            args.push("-to".into());
            args.push(format!("{e:.3}"));
        }
    }
    args.push("-i".into());
    args.push(video.to_string_lossy().to_string());

    // 字幕：ass 烧录（有 libass）> PNG 叠层烧录（片段兜底）> mov_text 软字幕（全片兜底）
    let mut soft_sub = false;
    if let Some(ass) = &ass_content {
        let ass_path = media_dir.join(format!("export_{video_id}.ass"));
        tokio::fs::write(&ass_path, ass).await.map_err(|e| e.to_string())?;
        let fonts_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("export_fonts");
        let vf = format!(
            "ass={}:fontsdir={}",
            esc_filter_path(&ass_path.to_string_lossy()),
            esc_filter_path(&fonts_dir.to_string_lossy())
        );
        args.push("-vf".into());
        args.push(vf);
    } else if !overlays.is_empty() {
        let ov_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("export_overlay")
            .join(video_id.to_string());
        let _ = tokio::fs::remove_dir_all(&ov_dir).await;
        tokio::fs::create_dir_all(&ov_dir)
            .await
            .map_err(|e| e.to_string())?;
        let mut script = String::from("[0:v]format=rgba[v0];\n");
        for (i, ov) in overlays.iter().enumerate() {
            let png_path = ov_dir.join(format!("cue_{i:03}.png"));
            tokio::fs::write(&png_path, &ov.png)
                .await
                .map_err(|e| e.to_string())?;
            args.push("-loop".into());
            args.push("1".into());
            args.push("-framerate".into());
            args.push("30".into());
            args.push("-i".into());
            args.push(png_path.to_string_lossy().to_string());
            script.push_str(&format!(
                "[v{i}][{n}:v]overlay=0:0:eof_action=pass:enable='between(t,{s:.3},{e:.3})'[v{n}];\n",
                n = i + 1,
                s = ov.start,
                e = ov.end,
            ));
        }
        script.push_str(&format!("[v{}]format=yuv420p[vout]\n", overlays.len()));
        let script_path = media_dir.join(format!("export_{video_id}.ffgraph"));
        tokio::fs::write(&script_path, script)
            .await
            .map_err(|e| e.to_string())?;
        args.push("-filter_complex_script".into());
        args.push(script_path.to_string_lossy().to_string());
        args.push("-map".into());
        args.push("[vout]".into());
        args.push("-map".into());
        args.push("0:a".into());
    } else if let Some(srt) = &srt_content {
        let srt_path = media_dir.join(format!("export_{video_id}.srt"));
        tokio::fs::write(&srt_path, srt).await.map_err(|e| e.to_string())?;
        args.push("-i".into());
        args.push(srt_path.to_string_lossy().to_string());
        args.push("-map".into());
        args.push("0:v".into());
        args.push("-map".into());
        args.push("0:a".into());
        args.push("-map".into());
        args.push("1:s".into());
        soft_sub = true;
    }

    args.extend([
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "veryfast".into(),
        "-crf".into(),
        "20".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "160k".into(),
    ]);
    if soft_sub {
        args.push("-c:s".into());
        args.push("mov_text".into());
    }
    args.extend([
        "-movflags".into(),
        "+faststart".into(),
        "-progress".into(),
        "pipe:1".into(),
        "-nostats".into(),
        "-nostdin".into(),
        "-y".into(),
        out_path.clone(),
    ]);

    let child = Command::new(&ffmpeg)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "找不到 ffmpeg，请在设置页配置路径".to_string()
            } else {
                e.to_string()
            }
        })?;

    let child = Arc::new(tokio::sync::Mutex::new(child));
    *state.0.lock().await = Some(child.clone());

    let stdout = child.lock().await.stdout.take().expect("stdout piped");
    let stderr = child.lock().await.stderr.take().expect("stderr piped");

    // -progress pipe:1 输出 out_time_ms=（微秒）→ 单调递增百分比
    let total = total_secs.max(0.001);
    let stdout_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut last = -1.0f64;
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(us) = line
                .strip_prefix("out_time_ms=")
                .and_then(|v| v.parse::<f64>().ok())
            {
                let pct = (us / 1_000_000.0 / total * 100.0).clamp(0.0, 99.0);
                if pct > last + 0.2 {
                    last = pct;
                    let _ = app.emit(
                        "export-progress",
                        ExportProgress {
                            video_id,
                            percent: (pct * 10.0).round() / 10.0,
                        },
                    );
                }
            }
        }
    });

    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut buf = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            buf.push_str(&line);
            buf.push('\n');
        }
        buf
    });

    // 轮询等待退出：cancel_export 可在间隙拿到锁杀掉进程
    let status = loop {
        {
            let mut guard = child.lock().await;
            match guard.try_wait() {
                Ok(Some(st)) => break st,
                Ok(None) => {}
                Err(e) => {
                    *state.0.lock().await = None;
                    return Err(e.to_string());
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    };
    *state.0.lock().await = None;

    let _ = stdout_task.await;
    let stderr = stderr_task.await.unwrap_or_default();

    if !status.success() {
        if Path::new(&out_path).exists() {
            let _ = tokio::fs::remove_file(&out_path).await;
        }
        let tail: String = stderr
            .lines()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        return Err(if tail.trim().is_empty() {
            "ffmpeg 导出失败".to_string()
        } else {
            tail
        });
    }
    Ok(())
}

#[tauri::command]
pub async fn cancel_export(state: tauri::State<'_, ExportState>) -> Result<(), String> {
    let guard = state.0.lock().await;
    if let Some(child) = guard.as_ref() {
        child.lock().await.start_kill().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 导出用字体同步：webview fetch 到的字体字节写入 app_data_dir/export_fonts（存在即跳过）
#[tauri::command]
pub async fn ensure_export_fonts(
    app: AppHandle,
    fonts: Vec<(String, Vec<u8>)>,
) -> Result<String, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("export_fonts");
    tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
    for (name, bytes) in fonts {
        let Some(file) = Path::new(&name).file_name() else {
            continue;
        };
        let path = dir.join(file);
        if path.exists() {
            continue;
        }
        tokio::fs::write(&path, &bytes)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(dir.to_string_lossy().to_string())
}

fn list_storyboard(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.starts_with("sb_") && n.ends_with(".jpg"))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out.into_iter()
        .map(|n| dir.join(n).to_string_lossy().to_string())
        .collect()
}

/// 时间轴/剪辑条共用的等距缩略图（storyboard）。
/// 关键帧解码（-skip_frame nokey）使长视频也能秒级出图：95min 视频实测 ~8s/360 帧。
/// interval 公式前后端共用，缓存目录数量达标即直接复用。
#[derive(Debug, Serialize)]
pub struct Storyboard {
    pub interval: f64,
    pub paths: Vec<String>,
}

pub fn storyboard_interval(duration_secs: f64) -> f64 {
    (duration_secs / 360.0).clamp(8.0, 30.0)
}

#[tauri::command]
pub async fn video_storyboard(
    app: AppHandle,
    pool: tauri::State<'_, SqlitePool>,
    video_id: i64,
    duration_secs: f64,
) -> Result<Storyboard, String> {
    let interval = storyboard_interval(duration_secs);
    let count = (duration_secs / interval).ceil() as i64;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("storyboard")
        .join(video_id.to_string());
    let cached = list_storyboard(&dir);
    if cached.len() as i64 >= count && count > 0 {
        return Ok(Storyboard { interval, paths: cached });
    }
    let _ = tokio::fs::remove_dir_all(&dir).await;
    tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;

    let video = video_path(&pool, video_id).await?;
    let ffmpeg = ffmpeg_bin(&pool).await;
    let args = vec![
        "-loglevel".into(),
        "error".into(),
        "-skip_frame".into(),
        "nokey".into(),
        "-i".into(),
        video.to_string_lossy().to_string(),
        "-vf".into(),
        format!("fps=1/{interval:.3},scale=160:90"),
        "-frames:v".into(),
        count.to_string(),
        "-q:v".into(),
        "4".into(),
        "-y".into(),
        dir.join("sb_%04d.jpg").to_string_lossy().to_string(),
    ];
    let output = Command::new(&ffmpeg)
        .args(&args)
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "找不到 ffmpeg，请在设置页配置路径".to_string()
            } else {
                e.to_string()
            }
        })?;
    if !output.status.success() {
        return Err(format!(
            "生成缩略图失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(Storyboard {
        interval,
        paths: list_storyboard(&dir),
    })
}

/// 在 Finder 中显示并选中文件
#[tauri::command]
pub fn reveal_in_folder(app: AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_filter_path() {
        assert_eq!(esc_filter_path("/tmp/a b/clip.ass"), "'/tmp/a b/clip.ass'");
        assert_eq!(
            esc_filter_path("/weird/o'hare/x.ass"),
            "'/weird/o'\\''hare/x.ass'"
        );
        assert_eq!(
            esc_filter_path("/Library/Application Support/x"),
            "'/Library/Application Support/x'"
        );
    }
}
