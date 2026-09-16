//! YouTube 频道：yt-dlp flat-playlist 拉取频道资料与最新视频（分页），频道订阅 CRUD。

use crate::commands::ytdlp_config;
use crate::ingest::{self, IngestError};
use crate::models::Subscription;
use sqlx::SqlitePool;

pub const PAGE_SIZE: i64 = 10;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChannelInfo {
    pub channel_id: String,
    pub title: String,
    pub url: String,
    pub avatar: Option<String>,
    pub description: Option<String>,
    pub follower_count: Option<i64>,
    pub verified: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChannelVideo {
    pub id: String,
    pub url: String,
    pub title: String,
    pub duration: Option<f64>,
    pub view_count: Option<i64>,
    pub thumbnail: Option<String>,
    pub live_status: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct ChannelPage {
    pub channel: ChannelInfo,
    pub videos: Vec<ChannelVideo>,
    pub page: i64,
    pub has_more: bool,
}

/// 频道 URL 归一化到具体 tab（无 tab 段时补 /videos），yt-dlp 才能稳定列出视频
fn channel_videos_url(url: &str) -> String {
    let u = url.trim().trim_end_matches('/');
    const TABS: [&str; 8] = [
        "/videos",
        "/shorts",
        "/streams",
        "/playlists",
        "/about",
        "/community",
        "/channels",
        "/featured",
    ];
    if TABS.iter().any(|t| u.ends_with(t)) {
        u.to_string()
    } else {
        format!("{u}/videos")
    }
}

#[derive(Debug, serde::Deserialize)]
struct RawThumb {
    url: String,
    width: Option<i64>,
}

#[derive(Debug, serde::Deserialize)]
struct RawEntry {
    id: Option<String>,
    url: Option<String>,
    title: Option<String>,
    duration: Option<f64>,
    view_count: Option<i64>,
    thumbnails: Option<Vec<RawThumb>>,
    live_status: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct RawPlaylist {
    channel: Option<String>,
    channel_id: Option<String>,
    uploader: Option<String>,
    uploader_url: Option<String>,
    channel_url: Option<String>,
    description: Option<String>,
    channel_follower_count: Option<i64>,
    channel_is_verified: Option<bool>,
    thumbnails: Option<Vec<RawThumb>>,
    title: Option<String>,
    entries: Option<Vec<RawEntry>>,
}

/// 频道头像：取最宽一张
fn widest(thumbs: &Option<Vec<RawThumb>>) -> Option<String> {
    thumbs
        .as_ref()?
        .iter()
        .max_by_key(|t| t.width.unwrap_or(0))
        .map(|t| t.url.clone())
}

/// 视频封面：优先 width≥320 的第一张，否则首张
fn entry_thumb(thumbs: &Option<Vec<RawThumb>>) -> Option<String> {
    let ts = thumbs.as_ref()?;
    ts.iter()
        .find(|t| t.width.unwrap_or(0) >= 320)
        .or(ts.first())
        .map(|t| t.url.clone())
}

/// 从 /channel/UCxxx 形式的 URL 提取频道 ID
fn extract_channel_id(url: &str) -> Option<String> {
    let (_, id) = url.rsplit_once("/channel/")?;
    let id = id.trim_matches('/');
    (!id.is_empty()).then(|| id.to_string())
}

/// yt-dlp 输出 → ChannelPage（纯函数便于测试）
fn parse_channel_page(raw: &str, url: &str, page: i64) -> Result<ChannelPage, String> {
    let line = raw.lines().next().unwrap_or("");
    let pl: RawPlaylist =
        serde_json::from_str(line).map_err(|e| format!("解析频道数据失败: {e}"))?;

    let title = pl
        .channel
        .or(pl.uploader)
        .or_else(|| {
            pl.title
                .map(|t| t.trim_end_matches(" - Videos").trim().to_string())
        })
        .unwrap_or_else(|| "未知频道".into());
    let ch_url = pl
        .channel_url
        .or(pl.uploader_url)
        .unwrap_or_else(|| url.into());
    // channel_id 是订阅去重键，必须非空稳定：缺失时从 channel_url 提取，兜底用频道 URL
    let channel_id = pl
        .channel_id
        .filter(|s| !s.is_empty())
        .or_else(|| extract_channel_id(&ch_url))
        .unwrap_or_else(|| ch_url.clone());
    let channel = ChannelInfo {
        channel_id,
        title,
        url: ch_url,
        avatar: widest(&pl.thumbnails),
        description: pl.description,
        follower_count: pl.channel_follower_count,
        verified: pl.channel_is_verified.unwrap_or(false),
    };
    let videos: Vec<ChannelVideo> = pl
        .entries
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| {
            let id = e.id?;
            Some(ChannelVideo {
                url: e
                    .url
                    .unwrap_or_else(|| format!("https://www.youtube.com/watch?v={id}")),
                id,
                title: e.title.unwrap_or_default(),
                duration: e.duration,
                view_count: e.view_count,
                thumbnail: entry_thumb(&e.thumbnails),
                live_status: e.live_status.filter(|s| s != "not_live"),
            })
        })
        .collect();
    let has_more = videos.len() as i64 >= PAGE_SIZE;
    Ok(ChannelPage {
        channel,
        videos,
        page,
        has_more,
    })
}

#[tauri::command]
pub async fn fetch_channel(
    pool: tauri::State<'_, SqlitePool>,
    url: String,
    page: Option<i64>,
) -> Result<ChannelPage, String> {
    let page = page.unwrap_or(1).max(1);
    let url = channel_videos_url(&url);
    let yt = ytdlp_config(&pool).await;
    let start = (page - 1) * PAGE_SIZE + 1;
    let end = page * PAGE_SIZE;
    let mut args = vec![
        "--flat-playlist".into(),
        "--dump-single-json".into(),
        "--no-warnings".into(),
        "--playlist-items".into(),
        format!("{start}-{end}"),
    ];
    args.extend(yt.auth_args());
    args.push(url.clone());
    let out = ingest::run(&yt.bin(), &args)
        .await
        .map_err(|e| match &e {
            IngestError::ToolNotFound(_) => e.to_string(),
            _ => format!("拉取频道失败: {e}"),
        })?;
    parse_channel_page(&out, &url, page)
}

// ---------- 订阅 ----------

#[tauri::command]
pub async fn subscribe_channel(
    pool: tauri::State<'_, SqlitePool>,
    channel: ChannelInfo,
) -> Result<i64, String> {
    // 已订阅则刷新资料并返回现有 id
    if let Some((id,)) = sqlx::query_as::<_, (i64,)>(
        "SELECT id FROM subscriptions WHERE channel_id = ?",
    )
    .bind(&channel.channel_id)
    .fetch_optional(pool.inner())
    .await
    .map_err(|e| e.to_string())?
    {
        sqlx::query(
            "UPDATE subscriptions SET title = ?, url = ?, avatar = ?, follower_count = ? WHERE id = ?",
        )
        .bind(&channel.title)
        .bind(&channel.url)
        .bind(&channel.avatar)
        .bind(channel.follower_count)
        .bind(id)
        .execute(pool.inner())
        .await
        .map_err(|e| e.to_string())?;
        return Ok(id);
    }
    let id = sqlx::query(
        "INSERT INTO subscriptions (channel_id, title, url, avatar, follower_count) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&channel.channel_id)
    .bind(&channel.title)
    .bind(&channel.url)
    .bind(&channel.avatar)
    .bind(channel.follower_count)
    .execute(pool.inner())
    .await
    .map_err(|e| e.to_string())?
    .last_insert_rowid();
    Ok(id)
}

#[tauri::command]
pub async fn unsubscribe_channel(
    pool: tauri::State<'_, SqlitePool>,
    channel_id: String,
) -> Result<(), String> {
    sqlx::query("DELETE FROM subscriptions WHERE channel_id = ?")
        .bind(&channel_id)
        .execute(pool.inner())
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn list_subscriptions(
    pool: tauri::State<'_, SqlitePool>,
) -> Result<Vec<Subscription>, String> {
    sqlx::query_as::<_, Subscription>(
        "SELECT * FROM subscriptions ORDER BY created_at DESC",
    )
    .fetch_all(pool.inner())
    .await
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_channel_url() {
        assert_eq!(
            channel_videos_url("https://www.youtube.com/@allin"),
            "https://www.youtube.com/@allin/videos"
        );
        assert_eq!(
            channel_videos_url("https://www.youtube.com/@allin/"),
            "https://www.youtube.com/@allin/videos"
        );
        assert_eq!(
            channel_videos_url("https://www.youtube.com/@allin/shorts"),
            "https://www.youtube.com/@allin/shorts"
        );
        assert_eq!(
            channel_videos_url("https://www.youtube.com/channel/UCESLZhusAkFfsNsApnjF_Cg"),
            "https://www.youtube.com/channel/UCESLZhusAkFfsNsApnjF_Cg/videos"
        );
    }

    // 真机 yt-dlp @allin 输出的裁剪样本
    const SAMPLE: &str = r#"{"channel": "All-In Podcast", "channel_id": "UCESLZhusAkFfsNsApnjF_Cg", "uploader": "All-In Podcast", "channel_url": "https://www.youtube.com/@allin", "description": "Poker. Business. Politics.", "channel_follower_count": 1130000, "channel_is_verified": true, "thumbnails": [{"url": "https://yt3.googleusercontent.com/small", "width": 88, "height": 88}, {"url": "https://yt3.googleusercontent.com/big", "width": 900, "height": 900}], "title": "All-In Podcast - Videos", "entries": [{"id": "PUcooQRy0PU", "url": "https://www.youtube.com/watch?v=PUcooQRy0PU", "title": "JD Vance on AI, Entitlement Fraud, Iran War", "duration": 1652, "view_count": 110000, "thumbnails": [{"url": "https://i.ytimg.com/vi/PUcooQRy0PU/hqdefault.jpg", "height": 94, "width": 168}, {"url": "https://i.ytimg.com/vi/PUcooQRy0PU/hq720.jpg", "height": 202, "width": 360}], "live_status": null}]}"#;

    #[test]
    fn parses_channel_page() {
        let page = parse_channel_page(SAMPLE, "https://www.youtube.com/@allin/videos", 1).unwrap();
        assert_eq!(page.channel.channel_id, "UCESLZhusAkFfsNsApnjF_Cg");
        assert_eq!(page.channel.title, "All-In Podcast");
        assert_eq!(page.channel.follower_count, Some(1130000));
        assert!(page.channel.verified);
        assert_eq!(
            page.channel.avatar.as_deref(),
            Some("https://yt3.googleusercontent.com/big")
        );
        assert_eq!(page.videos.len(), 1);
        assert!(!page.has_more);
        let v = &page.videos[0];
        assert_eq!(v.id, "PUcooQRy0PU");
        assert_eq!(v.duration, Some(1652.0));
        assert_eq!(
            v.thumbnail.as_deref(),
            Some("https://i.ytimg.com/vi/PUcooQRy0PU/hq720.jpg")
        );
        assert_eq!(v.live_status, None);
    }

    #[test]
    fn falls_back_channel_id_when_missing() {
        // 缺 channel_id 但有 /channel/UCxxx 形式 channel_url：提取 UC id
        let raw = r#"{"channel": "Foo", "channel_url": "https://www.youtube.com/channel/UCabcdef123456", "entries": []}"#;
        let page = parse_channel_page(raw, "https://www.youtube.com/@foo/videos", 1).unwrap();
        assert_eq!(page.channel.channel_id, "UCabcdef123456");
        // channel_id/channel_url 都缺：用频道页面 URL 作稳定键
        let raw = r#"{"channel": "Bar", "entries": []}"#;
        let page = parse_channel_page(raw, "https://www.youtube.com/@bar/videos", 1).unwrap();
        assert_eq!(page.channel.channel_id, "https://www.youtube.com/@bar/videos");
        // channel_id 为空字符串视为缺失
        let raw = r#"{"channel": "Baz", "channel_id": "", "channel_url": "https://www.youtube.com/channel/UCzzz", "entries": []}"#;
        let page = parse_channel_page(raw, "https://www.youtube.com/@baz/videos", 1).unwrap();
        assert_eq!(page.channel.channel_id, "UCzzz");
    }
}
