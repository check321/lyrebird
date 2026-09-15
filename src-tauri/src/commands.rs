use crate::ingest::{self, YtDlp};
use crate::models::{Cue, Video};
use crate::subtitle;
use serde::Serialize;
use sqlx::SqlitePool;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize)]
pub struct ImportProgress {
    /// 当前步骤序号（0 起）
    pub step: i32,
    pub total_steps: i32,
    pub label: String,
    /// 当前步骤内的百分比（无则不确定进度）
    pub percent: Option<f64>,
}

const IMPORT_STEPS: i32 = 4;

async fn setting(pool: &SqlitePool, key: &str) -> Option<String> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()?;
    row.map(|r| r.0).filter(|s| !s.trim().is_empty())
}

async fn ytdlp_config(pool: &SqlitePool) -> YtDlp {
    YtDlp {
        bin: setting(pool, "ytdlp_path").await,
        cookies_browser: setting(pool, "ytdlp_cookies_browser").await,
        cookies_file: setting(pool, "ytdlp_cookies_file").await,
    }
}

#[tauri::command]
pub async fn import_video(
    pool: tauri::State<'_, SqlitePool>,
    media_dir: tauri::State<'_, PathBuf>,
    app: AppHandle,
    url: String,
) -> Result<Video, String> {
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err("链接不能为空".into());
    }
    let emit = |step: i32, label: &str, percent: Option<f64>| {
        let _ = app.emit(
            "import-progress",
            ImportProgress {
                step,
                total_steps: IMPORT_STEPS,
                label: label.to_string(),
                percent,
            },
        );
    };

    // 已导入过则直接返回
    if let Some(v) = sqlx::query_as::<_, Video>("SELECT * FROM videos WHERE url = ?")
        .bind(&url)
        .fetch_optional(pool.inner())
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(v);
    }

    let yt = ytdlp_config(&pool).await;

    emit(0, "获取视频信息", None);
    let info = ingest::fetch_info(&url, &yt)
        .await
        .map_err(|e| e.to_string())?;

    let category = info
        .channel
        .clone()
        .or_else(|| info.uploader.clone())
        .unwrap_or_else(|| "未分类".into());
    let video_id: i64 = sqlx::query(
        "INSERT INTO videos (url, youtube_id, title, duration_secs, channel, channel_id, description, category) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&url)
    .bind(&info.id)
    .bind(&info.title)
    .bind(info.duration)
    .bind(&info.channel)
    .bind(&info.channel_id)
    .bind(&info.description)
    .bind(&category)
    .execute(pool.inner())
    .await
    .map_err(|e| e.to_string())?
    .last_insert_rowid();

    let app2 = app.clone();
    let (video_path, sub_path, thumb_path) = ingest::download(
        &url,
        &info.id,
        &media_dir,
        &yt,
        move |stage, pct| {
            let _ = app2.emit(
                "import-progress",
                ImportProgress {
                    step: 1,
                    total_steps: IMPORT_STEPS,
                    label: stage.to_string(),
                    percent: pct,
                },
            );
        },
    )
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("Sign in to confirm") || msg.contains("cookies") {
            "YouTube 要求人机校验。请在设置页配置「Cookies 来源」（推荐：从浏览器读取）后重试。".to_string()
        } else {
            msg
        }
    })?;

    let (subtitle_source, audio_path) = match sub_path {
        Some(sub) => {
            emit(2, "解析字幕", None);
            let content = tokio::fs::read_to_string(&sub)
                .await
                .map_err(|e| e.to_string())?;
            let cues = subtitle::parse(&content);
            let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
            for (idx, c) in cues.iter().enumerate() {
                sqlx::query(
                    "INSERT INTO cues (video_id, idx, start_secs, end_secs, text_en) VALUES (?, ?, ?, ?, ?)",
                )
                .bind(video_id)
                .bind(idx as i64)
                .bind(c.start_secs)
                .bind(c.end_secs)
                .bind(&c.text)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            }
            tx.commit().await.map_err(|e| e.to_string())?;
            ("youtube", None)
        }
        None => {
            emit(2, "提取音轨（待 ASR 转写）", None);
            let wav = media_dir.join(format!("{}.wav", info.id));
            let ffmpeg = setting(&pool, "ffmpeg_path").await;
            let audio = ingest::extract_audio(&video_path, &wav, ffmpeg.as_deref())
                .await
                .map_err(|e| e.to_string())?;
            ("asr", Some(audio.to_string_lossy().to_string()))
        }
    };

    sqlx::query(
        "UPDATE videos SET video_path = ?, audio_path = ?, subtitle_source = ?, thumbnail_path = ? WHERE id = ?",
    )
    .bind(video_path.to_string_lossy().to_string())
    .bind(audio_path)
    .bind(subtitle_source)
    .bind(thumb_path.map(|p| p.to_string_lossy().to_string()))
    .bind(video_id)
    .execute(pool.inner())
    .await
    .map_err(|e| e.to_string())?;

    emit(3, "完成", Some(100.0));
    sqlx::query_as::<_, Video>("SELECT * FROM videos WHERE id = ?")
        .bind(video_id)
        .fetch_one(pool.inner())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_video(
    pool: tauri::State<'_, SqlitePool>,
    media_dir: tauri::State<'_, PathBuf>,
    id: i64,
) -> Result<(), String> {
    let yt_id: Option<(Option<String>,)> =
        sqlx::query_as("SELECT youtube_id FROM videos WHERE id = ?")
            .bind(id)
            .fetch_optional(pool.inner())
            .await
            .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM videos WHERE id = ?")
        .bind(id)
        .execute(pool.inner())
        .await
        .map_err(|e| e.to_string())?;

    // 清理媒体目录里该视频的所有文件（视频/音轨/字幕）
    if let Some((Some(yt),)) = yt_id {
        if let Ok(rd) = std::fs::read_dir(media_dir.inner()) {
            for entry in rd.flatten() {
                if entry.file_name().to_string_lossy().starts_with(&yt) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn update_video(
    pool: tauri::State<'_, SqlitePool>,
    id: i64,
    title: Option<String>,
    category: Option<String>,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE videos SET title = COALESCE(?, title), category = COALESCE(NULLIF(?, ''), category) WHERE id = ?",
    )
    .bind(title)
    .bind(category)
    .bind(id)
    .execute(pool.inner())
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn rename_category(
    pool: tauri::State<'_, SqlitePool>,
    old_name: String,
    new_name: String,
) -> Result<u64, String> {
    if new_name.trim().is_empty() {
        return Err("分类名不能为空".into());
    }
    let n = sqlx::query("UPDATE videos SET category = ? WHERE category = ?")
        .bind(new_name.trim())
        .bind(&old_name)
        .execute(pool.inner())
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();
    Ok(n)
}

#[tauri::command]
pub async fn get_video(
    pool: tauri::State<'_, SqlitePool>,
    id: i64,
) -> Result<Option<Video>, String> {
    sqlx::query_as::<_, Video>("SELECT * FROM videos WHERE id = ?")
        .bind(id)
        .fetch_optional(pool.inner())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_cues(pool: tauri::State<'_, SqlitePool>, video_id: i64) -> Result<Vec<Cue>, String> {
    sqlx::query_as::<_, Cue>("SELECT * FROM cues WHERE video_id = ? ORDER BY idx")
        .bind(video_id)
        .fetch_all(pool.inner())
        .await
        .map_err(|e| e.to_string())
}
