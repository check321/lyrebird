//! WebVTT / SRT 解析为归一化 cue 列表。

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCue {
    pub start_secs: f64,
    pub end_secs: f64,
    pub text: String,
}

/// 根据内容自动判断 VTT 或 SRT 格式并解析。
pub fn parse(content: &str) -> Vec<ParsedCue> {
    let mut cues = Vec::new();
    // 时间轴行：00:00:01.000 --> 00:00:04.000（VTT 可带位置参数）
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if let Some((start, end)) = parse_timestamp_line(line) {
            let mut text = String::new();
            i += 1;
            while i < lines.len() && !lines[i].trim().is_empty() {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(strip_tags(lines[i].trim()).as_str());
                i += 1;
            }
            if !text.is_empty() {
                cues.push(ParsedCue {
                    start_secs: start,
                    end_secs: end,
                    text,
                });
            }
        }
        i += 1;
    }
    dedup_rolling(cues)
}

/// YouTube 自动字幕是"滚动"式的：每条 cue 会重复上一条的尾部内容。
/// 按词去掉与前一条原文重叠的前缀，完全重叠的 cue 直接丢弃。
fn dedup_rolling(cues: Vec<ParsedCue>) -> Vec<ParsedCue> {
    let mut out: Vec<ParsedCue> = Vec::new();
    let mut prev_words: Vec<String> = Vec::new();
    for c in cues {
        let cur_words = word_tokens(&c.text);
        let max = prev_words.len().min(cur_words.len());
        // 找最大的 k：cur 的前 k 个词 == prev 的后 k 个词
        let mut k = 0;
        for n in (1..=max).rev() {
            if cur_words[..n] == prev_words[prev_words.len() - n..] {
                k = n;
                break;
            }
        }
        if k == cur_words.len() && !cur_words.is_empty() {
            // 整条都是上一条的重复，跳过（但保留时间覆盖：延长上一条）
            if let Some(last) = out.last_mut() {
                last.end_secs = last.end_secs.max(c.end_secs);
            }
            continue;
        }
        let text = strip_first_words(&c.text, k).trim().to_string();
        prev_words = cur_words;
        if text.is_empty() {
            continue;
        }
        out.push(ParsedCue { text, ..c });
    }
    out
}

/// 提取词 token（小写、仅字母数字撇号），用于重叠比较。
fn word_tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

/// 删除文本开头的前 n 个词，保留剩余原始文本（含标点）。
fn strip_first_words(s: &str, n: usize) -> String {
    let mut count = 0;
    let mut in_word = false;
    for (i, c) in s.char_indices() {
        let is_word = c.is_alphanumeric() || c == '\'';
        if is_word && !in_word {
            count += 1;
            if count > n {
                return s[i..].to_string();
            }
        }
        in_word = is_word;
    }
    String::new()
}

fn parse_timestamp_line(line: &str) -> Option<(f64, f64)> {
    let (left, right) = line.split_once("-->")?;
    // VTT 结束时间后可能跟位置设置，只取第一个 token
    let end_str = right.split_whitespace().next()?;
    Some((parse_ts(left.trim())?, parse_ts(end_str)?))
}

fn parse_ts(s: &str) -> Option<f64> {
    let s = s.replace(',', ".");
    let mut parts = s.split(':').collect::<Vec<_>>();
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    let secs: f64 = parts.pop()?.parse().ok()?;
    let mins: f64 = parts.pop()?.parse().ok()?;
    let hours: f64 = if parts.is_empty() {
        0.0
    } else {
        parts.pop()?.parse().ok()?
    };
    Some(hours * 3600.0 + mins * 60.0 + secs)
}

/// 去掉 VTT 内联标签（<c>、<00:00:01.000>、<b> 等）。
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_srt() {
        let srt = "1\n00:00:01,000 --> 00:00:04,000\nHello world\n\n2\n00:00:05,500 --> 00:00:07,000\nSecond line\n";
        let cues = parse(srt);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start_secs, 1.0);
        assert_eq!(cues[0].end_secs, 4.0);
        assert_eq!(cues[0].text, "Hello world");
        assert_eq!(cues[1].start_secs, 5.5);
    }

    #[test]
    fn parse_vtt_with_tags() {
        let vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:04.000 align:start position:0%\n<c>Hello</c> <00:00:02.000><c>world</c>\n\n";
        let cues = parse(vtt);
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "Hello world");
    }

    #[test]
    fn dedup_rolling_captions() {
        // YouTube 自动字幕的滚动重复
        let srt = "1\n00:00:01,000 --> 00:00:02,000\nThe scop cance canceled on us. Is that\n\n2\n00:00:02,000 --> 00:00:03,000\nThe scop cance canceled on us. Is that what happened?\n\n3\n00:00:03,000 --> 00:00:04,000\nwhat happened?\n\n4\n00:00:04,000 --> 00:00:05,000\nwhat happened? Yeah, the scop cance canceled. He\n";
        let cues = parse(srt);
        assert_eq!(cues.len(), 3);
        assert_eq!(cues[0].text, "The scop cance canceled on us. Is that");
        assert_eq!(cues[1].text, "what happened?");
        // 整条重复的 cue 被合并掉，但延长上一条的结束时间
        assert_eq!(cues[1].end_secs, 4.0);
        assert_eq!(cues[2].text, "Yeah, the scop cance canceled. He");
    }
}
