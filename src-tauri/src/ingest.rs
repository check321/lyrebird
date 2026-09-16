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
    pub(crate) fn bin(&self) -> String {
        self.bin.clone().unwrap_or_else(|| "yt-dlp".into())
    }

    pub(crate) fn auth_args(&self) -> Vec<String> {
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

pub(crate) async fn run(bin: &str, args: &[String]) -> Result<String, IngestError> {
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

/// 下载阶段进度
#[derive(Debug, Clone, Default)]
pub struct DlProgress {
    /// 阶段描述（下载视频/合并音视频/…）
    pub stage: &'static str,
    /// 百分比（单调钳制，供总进度条）；大小未知则无
    pub percent: Option<f64>,
    /// 实时网速，如 "1.2MiB/s"
    pub speed: Option<String>,
    /// 字节级真实进度，如 "12.3MiB / 26.9MiB"（多趟下载按字节聚合，总大小已知时才有）
    pub detail: Option<String>,
}

impl DlProgress {
    fn stage_only(stage: &'static str) -> Self {
        DlProgress {
            stage,
            ..Default::default()
        }
    }
}

/// [download] 进度跟踪器：视频/音频/字幕分多趟下载，每趟百分比各自从 0 计，
/// 这里把各趟按字节聚合成整体进度；字幕、封面等小文件趟不计入。
#[derive(Default)]
struct DlTracker {
    /// 已完成各趟的总字节
    completed: f64,
    /// 当前趟已下载字节 / 总字节
    cur_dl: f64,
    cur_total: Option<f64>,
    /// 当前趟是字幕/封面等小文件，不上报也不计入合计
    skip_pass: bool,
}

impl DlTracker {
    fn new_pass(&mut self, dest: &str) {
        if !self.skip_pass {
            if let Some(t) = self.cur_total {
                self.completed += t;
            }
        }
        self.cur_dl = 0.0;
        self.cur_total = None;
        self.skip_pass = [".vtt", ".srt", ".ass", ".webp", ".jpg", ".jpeg", ".png"]
            .iter()
            .any(|e| dest.ends_with(e));
    }

    fn update(&mut self, dl: f64, total: Option<f64>) {
        self.cur_dl = dl;
        if total.is_some() {
            self.cur_total = total;
        }
    }

    /// 合计 (已下载字节, 总字节)；小文件趟返回 None
    fn combined(&self) -> Option<(f64, Option<f64>)> {
        if self.skip_pass {
            return None;
        }
        Some((
            self.completed + self.cur_dl,
            self.cur_total.map(|t| self.completed + t),
        ))
    }
}

/// 下载视频 + 英文字幕 + 封面到 media_dir。
/// `on_progress` 收到 DlProgress（阶段/百分比/网速/字节级进度）回调。
/// 断点续传：显式 --continue（防用户 yt-dlp 全局配置关掉），中断后保留 .part 文件，
/// 重跑同一输出模板自动从断点继续；完整文件则由 yt-dlp 自检跳过。
/// 返回 (视频路径, 字幕 SRT 路径, 封面路径)。
pub async fn download(
    url: &str,
    yt_id: &str,
    media_dir: &Path,
    yt: &YtDlp,
    on_progress: impl Fn(DlProgress) + Send + 'static,
) -> Result<(PathBuf, Option<PathBuf>, Option<PathBuf>), IngestError> {
    tokio::fs::create_dir_all(media_dir).await?;
    let out_tmpl = media_dir.join(format!("{yt_id}.%(ext)s"));
    let mut args = vec![
        "--no-playlist".into(),
        "--newline".into(),
        "--continue".into(),
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

    // 边下边解析 [download] 进度行：detail 展示聚合后的真实字节进度，
    // percent 取历史最大值（单调钳制，只供总进度条，音频趟开始时不会因分母变大而回退）。
    let stdout_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut tracker = DlTracker::default();
        let mut max_pct: Option<f64> = None;
        let mut emitted_pct = -1.0f64;
        let mut last_speed = String::new();
        let mut last_emit = std::time::Instant::now() - std::time::Duration::from_secs(1);
        while let Ok(Some(line)) = lines.next_line().await {
            if !line.contains("[download]") {
                if line.contains("[Merger]") || line.contains("Merging formats") {
                    on_progress(DlProgress::stage_only("合并音视频"));
                } else if line.contains("[VideoConvert]") || line.contains("[SubtitlesConvertor]")
                {
                    on_progress(DlProgress::stage_only("转换字幕格式"));
                }
                continue;
            }
            if let Some(idx) = line.find("Destination:") {
                tracker.new_pass(line[idx + "Destination:".len()..].trim());
                continue;
            }
            let pct = if line.contains('%') {
                parse_percent(&line)
            } else {
                None
            };
            let speed = parse_speed(&line);
            if let Some((dl, total)) = parse_sizes(&line) {
                tracker.update(dl, total);
            }
            // 字幕/封面趟不上报
            let Some((dl, total)) = tracker.combined() else {
                continue;
            };
            if pct.is_none() && speed.is_none() && total.is_none() {
                continue;
            }
            // 总字节已知用聚合百分比，否则退回本趟百分比（HLS 分片等）
            let step_pct = match total {
                Some(t) if t > 0.0 => Some(dl / t * 100.0),
                _ => pct,
            };
            if let Some(p) = step_pct {
                max_pct = Some(max_pct.map_or(p, |m| m.max(p)));
            }
            let detail = total
                .filter(|t| *t > 0.0)
                .map(|t| format!("{} / {}", fmt_size(dl), fmt_size(t)));
            let pct_changed = max_pct.is_some_and(|m| m - emitted_pct >= 0.5);
            let done = max_pct == Some(100.0) && emitted_pct < 100.0;
            let speed_due = speed.as_deref().is_some_and(|s| s != last_speed)
                && last_emit.elapsed() >= std::time::Duration::from_millis(300);
            if pct_changed || done || speed_due {
                emitted_pct = max_pct.unwrap_or(emitted_pct);
                if let Some(s) = &speed {
                    last_speed = s.clone();
                }
                last_emit = std::time::Instant::now();
                on_progress(DlProgress {
                    stage: "下载视频",
                    percent: max_pct,
                    speed,
                    detail,
                });
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

/// 从 [download] 行提取实时网速（形如 1.23MiB/s 的 token）
fn parse_speed(line: &str) -> Option<String> {
    line.split_whitespace()
        .find(|t| {
            t.ends_with("/s") && t.chars().next().is_some_and(|c| c.is_ascii_digit())
        })
        .map(str::to_string)
}

/// 解析 yt-dlp 字节数 token："360.50MiB" / "440.00B" → 字节
fn parse_size(tok: &str) -> Option<f64> {
    const UNITS: [(&str, f64); 5] = [
        ("TiB", 1099511627776.0),
        ("GiB", 1073741824.0),
        ("MiB", 1048576.0),
        ("KiB", 1024.0),
        ("B", 1.0),
    ];
    for (suffix, mult) in UNITS {
        if let Some(num) = tok.strip_suffix(suffix) {
            if let Ok(v) = num.parse::<f64>() {
                return Some(v * mult);
            }
        }
    }
    None
}

/// 字节 → "12.3MiB" 风格字符串
fn fmt_size(bytes: f64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut v = bytes;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{v:.0}{}", UNITS[i])
    } else {
        format!("{v:.1}{}", UNITS[i])
    }
}

/// 从 [download] 进度行解析 (已下载字节, 总字节)。总字节可能缺失（HLS 分片等）。
fn parse_sizes(line: &str) -> Option<(f64, Option<f64>)> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.first() != Some(&"[download]") {
        return None;
    }
    let t1 = tokens.get(1)?;
    if t1.ends_with('%') {
        // "45.3% of  360.50MiB at …" / "23.4% of ~  98.00MiB at …"；无 "of" 则无法算字节
        let of_idx = tokens.iter().position(|t| *t == "of")?;
        let size_tok = tokens[of_idx + 1..].iter().find(|t| **t != "~")?;
        let total = parse_size(size_tok)?;
        let pct: f64 = t1.trim_end_matches('%').parse().ok()?;
        Some((pct / 100.0 * total, Some(total)))
    } else {
        // "10.00MiB at …"（总大小未知，只有已下载量）
        parse_size(t1).map(|dl| (dl, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_download_progress_lines() {
        // 常规行：百分比 + 网速 + ETA
        let l = "[download]  45.3% of  360.50MiB at  1.07MiB/s ETA 05:32";
        assert_eq!(parse_percent(l), Some(45.3));
        assert_eq!(parse_speed(l).as_deref(), Some("1.07MiB/s"));
        // 总大小未知：无百分比，只有网速
        let l = "[download]  10.00MiB at  512.00KiB/s";
        assert_eq!(parse_speed(l).as_deref(), Some("512.00KiB/s"));
        // HLS 分片行：frag 计数不能被误判为网速
        let l = "[download]  23.4% of ~  98.00MiB at    2.66MiB/s ETA 01:05 (frag 24/106)";
        assert_eq!(parse_percent(l), Some(23.4));
        assert_eq!(parse_speed(l).as_deref(), Some("2.66MiB/s"));
        // 完成行：in + at 并存
        let l = "[download] 100% of  360.50MiB in 00:02:15 at 2.66MiB/s";
        assert_eq!(parse_percent(l), Some(100.0));
        assert_eq!(parse_speed(l).as_deref(), Some("2.66MiB/s"));
        // 非进度行
        assert_eq!(parse_speed("[Merger] Merging formats into x.mp4"), None);
    }

    #[test]
    fn parses_sizes_from_progress_lines() {
        // 百分比 + 总量 → 换算已下载字节
        let (dl, total) =
            parse_sizes("[download]  45.3% of  360.50MiB at  1.07MiB/s ETA 05:32").unwrap();
        assert_eq!(total, Some(360.50 * 1024.0 * 1024.0));
        assert!((dl - 0.453 * 360.50 * 1024.0 * 1024.0).abs() < 1.0);
        // "~" 估算总量
        let (_, total) =
            parse_sizes("[download]  23.4% of ~  98.00MiB at 2.66MiB/s ETA 01:05 (frag 24/106)")
                .unwrap();
        assert_eq!(total, Some(98.0 * 1024.0 * 1024.0));
        // 完成行
        let (dl, total) =
            parse_sizes("[download] 100% of  360.50MiB in 00:02:15 at 2.66MiB/s").unwrap();
        assert_eq!(dl, total.unwrap());
        // 总大小未知：只有已下载量
        let (dl, total) = parse_sizes("[download]  10.00MiB at  512.00KiB/s").unwrap();
        assert_eq!(dl, 10.0 * 1024.0 * 1024.0);
        assert_eq!(total, None);
        // 续传提示行 / 已下载完成行不算进度
        assert_eq!(parse_sizes("[download] Resuming download at byte 13672625"), None);
        assert_eq!(
            parse_sizes("[download] x.f133.mp4 has already been downloaded"),
            None
        );
    }

    #[test]
    fn tracks_multi_pass_bytes() {
        let mut t = DlTracker::default();
        // 字幕趟（下载视频前先下字幕）：不计入
        t.new_pass("x.en.vtt");
        let (dl, total) = parse_sizes("[download] 100% of    440.00B in 00:00:00").unwrap();
        t.update(dl, total);
        assert_eq!(t.combined(), None);
        // 视频趟
        t.new_pass("x.f133.mp4");
        let (dl, total) = parse_sizes("[download]  50.0% of  100.00MiB at 1MiB/s").unwrap();
        t.update(dl, total);
        let (dl, total) = t.combined().unwrap();
        assert_eq!(dl, 50.0 * 1024.0 * 1024.0);
        assert_eq!(total, Some(100.0 * 1024.0 * 1024.0));
        // 视频完成
        let (dl, total) = parse_sizes("[download] 100% of  100.00MiB in 00:01:00").unwrap();
        t.update(dl, total);
        // 音频趟开始：视频字节计入已完成，合计分母变大
        t.new_pass("x.f140.m4a");
        let (dl, total) = parse_sizes("[download]  50.0% of   10.00MiB at 1MiB/s").unwrap();
        t.update(dl, total);
        let (dl, total) = t.combined().unwrap();
        assert_eq!(dl, 105.0 * 1024.0 * 1024.0);
        assert_eq!(total, Some(110.0 * 1024.0 * 1024.0));
        // 封面趟：音频计入完成，封面自身不计入
        t.new_pass("x.webp");
        assert_eq!(t.combined(), None);
        assert_eq!(t.completed, 110.0 * 1024.0 * 1024.0);
    }

    #[test]
    fn formats_sizes() {
        assert_eq!(fmt_size(440.0), "440B");
        assert_eq!(fmt_size(512.0 * 1024.0), "512.0KiB");
        assert_eq!(fmt_size(26.9 * 1024.0 * 1024.0), "26.9MiB");
        assert_eq!(parse_size("2.50GiB"), Some(2.5 * 1024f64.powi(3)));
    }
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
