//! yt-dlp / ffmpeg 外部进程封装：下载视频、抓字幕、抽音轨。

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[derive(Debug, Deserialize)]
pub struct YtInfo {
    pub id: String,
    pub title: Option<String>,
    pub duration: Option<f64>,
    pub channel: Option<String>,
    pub channel_id: Option<String>,
    pub uploader: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error("找不到外部工具 {0}，请在设置页配置路径")]
    ToolNotFound(String),
    #[error("{0} 执行失败: {1}")]
    Failed(String, String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// yt-dlp 调用配置：二进制路径 + cookies 认证
#[derive(Debug, Clone, Default)]
pub struct YtDlp {
    pub bin: Option<String>,
    pub cookies_browser: Option<String>,
    pub cookies_file: Option<String>,
}

impl YtDlp {
    fn bin(&self) -> String {
        self.bin.clone().unwrap_or_else(|| "yt-dlp".into())
    }

    fn auth_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(b) = &self.cookies_browser {
            args.push("--cookies-from-browser".into());
            args.push(b.clone());
        } else if let Some(f) = &self.cookies_file {
            args.push("--cookies".into());
            args.push(f.clone());
        }
        args
    }
}

async fn run(bin: &str, args: &[String]) -> Result<String, IngestError> {
    let output = Command::new(bin).args(args).output().await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            IngestError::ToolNotFound(bin.to_string())
        } else {
            IngestError::Io(e)
        }
    })?;
    if !output.status.success() {
        return Err(IngestError::Failed(
            bin.to_string(),
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 拉取视频元数据（不下载）。
pub async fn fetch_info(url: &str, yt: &YtDlp) -> Result<YtInfo, IngestError> {
    let mut args = vec!["--dump-json".into(), "--no-playlist".into()];
    args.extend(yt.auth_args());
    args.push(url.into());
    let out = run(&yt.bin(), &args).await?;
    let line = out.lines().next().unwrap_or("");
    Ok(serde_json::from_str(line)?)
}

/// 下载视频 + 英文字幕 + 封面到 media_dir。
/// `on_progress` 收到 (阶段描述, 百分比 0-100) 回调。
/// 返回 (视频路径, 字幕 SRT 路径, 封面路径)。
pub async fn download(
    url: &str,
    yt_id: &str,
    media_dir: &Path,
    yt: &YtDlp,
    on_progress: impl Fn(&str, Option<f64>) + Send + 'static,
) -> Result<(PathBuf, Option<PathBuf>, Option<PathBuf>), IngestError> {
    tokio::fs::create_dir_all(media_dir).await?;
    let out_tmpl = media_dir.join(format!("{yt_id}.%(ext)s"));
    let mut args = vec![
        "--no-playlist".into(),
        "--newline".into(),
        // 优先 H.264(avc1) 视频 + AAC 音频：WKWebView 不支持 AV1/VP9
        "-f".into(),
        "bv*[vcodec^=avc1][ext=mp4]+ba[ext=m4a]/bv*[vcodec^=avc1]+ba/b[vcodec^=avc1]/b".into(),
        "--remux-video".into(),
        "mp4".into(),
        "--write-subs".into(),
        "--write-auto-subs".into(),
        "--write-thumbnail".into(),
        "--sub-langs".into(),
        "en.*".into(),
        "--convert-subs".into(),
        "srt".into(),
        "-o".into(),
        out_tmpl.to_string_lossy().to_string(),
    ];
    args.extend(yt.auth_args());
    args.push(url.into());

    let bin = yt.bin();
    let mut child = Command::new(&bin)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                IngestError::ToolNotFound(bin.clone())
            } else {
                IngestError::Io(e)
            }
        })?;

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    // 边下边解析 [download]  42.1% ... 进度行
    let stdout_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut last_pct = -1.0;
        while let Ok(Some(line)) = lines.next_line().await {
            if line.contains("[download]") && line.contains('%') {
                if let Some(pct) = parse_percent(&line) {
                    if (pct - last_pct).abs() >= 0.5 {
                        last_pct = pct;
                        on_progress("下载视频", Some(pct));
                    }
                }
            } else if line.contains("[Merger]") || line.contains("Merging formats") {
                on_progress("合并音视频", None);
            } else if line.contains("[VideoConvert]") || line.contains("[SubtitlesConvertor]") {
                on_progress("转换字幕格式", None);
            }
        }
    });

    let stderr_task = tokio::spawn(async move {
        let mut buf = String::new();
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.contains("ERROR") {
                buf.push_str(&line);
                buf.push('\n');
            }
        }
        buf
    });

    let status = child.wait().await?;
    let _ = stdout_task.await;
    let stderr = stderr_task.await.unwrap_or_default();
    if !status.success() {
        return Err(IngestError::Failed(
            bin,
            if stderr.is_empty() {
                "未知错误".into()
            } else {
                stderr.trim().to_string()
            },
        ));
    }

    let video = find_file(media_dir, yt_id, &["mp4", "mkv", "webm", "m4v"]).ok_or(
        IngestError::Failed(bin, "下载完成但找不到视频文件".into()),
    )?;
    let sub = find_file(media_dir, yt_id, &["srt"]);
    let thumb = find_file(media_dir, yt_id, &["webp", "jpg", "jpeg", "png"]);
    Ok((video, sub, thumb))
}

fn parse_percent(line: &str) -> Option<f64> {
    let idx = line.find('%')?;
    let before = &line[..idx];
    let num: String = before
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    num.parse().ok()
}

/// 从视频提取 16kHz 单声道 WAV，供 ASR 使用。
pub async fn extract_audio(video: &Path, out: &Path, ffmpeg: Option<&str>) -> Result<PathBuf, IngestError> {
    let bin = ffmpeg.unwrap_or("ffmpeg");
    let args = vec![
        "-y".into(),
        "-i".into(),
        video.to_string_lossy().to_string(),
        "-vn".into(),
        "-ac".into(),
        "1".into(),
        "-ar".into(),
        "16000".into(),
        out.to_string_lossy().to_string(),
    ];
    run(bin, &args).await?;
    Ok(out.to_path_buf())
}

fn find_file(dir: &Path, stem_prefix: &str, exts: &[&str]) -> Option<PathBuf> {
    let rd = std::fs::read_dir(dir).ok()?;
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(stem_prefix)
            && exts
                .iter()
                .any(|e| name.to_lowercase().ends_with(&format!(".{e}")))
        {
            return Some(entry.path());
        }
    }
    None
}
