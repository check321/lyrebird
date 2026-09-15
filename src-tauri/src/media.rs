//! 内置本地 HTTP 媒体服务器：WKWebView 走自定义 asset 协议播放大视频有兼容问题
//! （典型症状：有画面无声音），改用 127.0.0.1 上的 HTTP 服务（支持 Range 请求）。

use axum::Router;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use tower_http::services::ServeDir;

pub struct MediaPort(pub u16);

/// 启动媒体服务器，返回绑定端口。
pub async fn serve(media_dir: PathBuf) -> std::io::Result<MediaPort> {
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let port = listener.local_addr()?.port();
    tokio::spawn(async move {
        let router = Router::new().nest_service("/media", ServeDir::new(media_dir));
        let _ = axum::serve(listener, router).await;
    });
    Ok(MediaPort(port))
}

/// 由本地文件路径生成可播放的 HTTP URL。
#[tauri::command]
pub fn media_url(port: tauri::State<'_, MediaPort>, path: String) -> Result<String, String> {
    let name = Path::new(&path)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("非法文件路径")?;
    let encoded: String = name
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    Ok(format!("http://127.0.0.1:{}/media/{}", port.0, encoded))
}
