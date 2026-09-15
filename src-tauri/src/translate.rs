//! 翻译管线 / AI 断句分段 / TLDR。
//! 设计原则：整理（分段校正）与翻译完全分离——
//! 分段自动执行、追求最快（极简边界输出 + 并发）；
//! 翻译由用户触发；句级中文由 cue 区间本地拼接（fill_sentence_zh），不重复调用 LLM。

use crate::llm::{self, LlmConfig};
use futures::StreamExt;
use serde::Serialize;
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Serialize, Clone)]
pub struct TranslateProgress {
    pub video_id: i64,
    pub done: usize,
    pub total: usize,
    /// 本块刚完成的 (cue_id, 译文)，前端收到即可就地更新，实现渐进式翻译
    pub pairs: Vec<(i64, String)>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct CueRow {
    id: i64,
    text_en: String,
}

const SYSTEM_PROMPT: &str = r#"你是一位专业的播客翻译家，负责把英文播客字幕翻译成中文。
要求：
1. 信达雅：先准确理解语义，再用自然流畅的中文表达，符合母语者播客语境，不要逐字硬翻。
2. 保留原文中的人名、品牌、专业术语的惯用译法。
3. 输入是按编号给出的若干字幕句子，输出严格为 JSON：{"translations": ["译文1", "译文2", ...]}，数组长度和顺序必须与输入句子一一对应，不得合并或拆分。
4. 只输出 JSON，不要任何解释。"#;

#[tauri::command]
pub async fn translate_video(
    pool: tauri::State<'_, SqlitePool>,
    app: AppHandle,
    video_id: i64,
) -> Result<usize, String> {
    let cfg = LlmConfig::from_settings(&pool)
        .await
        .map_err(|e| e.to_string())?;

    let cues = sqlx::query_as::<_, CueRow>(
        "SELECT id, text_en FROM cues WHERE video_id = ? AND text_zh IS NULL ORDER BY idx",
    )
    .bind(video_id)
    .fetch_all(pool.inner())
    .await
    .map_err(|e| e.to_string())?;
    if cues.is_empty() {
        return Ok(0);
    }

    // 按字符数把 cue 聚成块（保持完整句子），每块 ≤ ~1200 字符；owned 数据便于并发
    let mut blocks: Vec<Vec<CueRow>> = Vec::new();
    let mut cur: Vec<CueRow> = Vec::new();
    let mut cur_len = 0;
    for c in cues {
        if cur_len + c.text_en.len() > 1200 && !cur.is_empty() {
            blocks.push(std::mem::take(&mut cur));
            cur_len = 0;
        }
        cur_len += c.text_en.len();
        cur.push(c);
    }
    if !cur.is_empty() {
        blocks.push(cur);
    }

    let total = blocks.len();

    // 3 路并发翻译；每块完成立即入库并推送进度事件（携带译文），前端边翻译边显示。
    // 单块失败不整体回滚：已译部分保留，剩余可重试续传（只取 text_zh IS NULL 的 cue）。
    let mut stream = futures::stream::iter(blocks.into_iter().map(|block| {
        let cfg = cfg.clone();
        async move {
            let numbered = block
                .iter()
                .enumerate()
                .map(|(j, c)| format!("{}. {}", j + 1, c.text_en))
                .collect::<Vec<_>>()
                .join("\n");
            let out = llm::chat(&cfg, SYSTEM_PROMPT, &numbered, true)
                .await
                .map_err(|e| e.to_string())?;

            // 模型可能把 JSON 包在 markdown 代码块里，提取第一个 { 到最后一个 }
            let json_text = out
                .find('{')
                .and_then(|s| out.rfind('}').map(|e| &out[s..=e]))
                .unwrap_or(&out);
            let parsed: serde_json::Value = serde_json::from_str(json_text)
                .map_err(|e| format!("LLM 输出解析失败: {e}"))?;
            let arr = parsed
                .get("translations")
                .and_then(|v| v.as_array())
                .ok_or("LLM 输出缺少 translations 数组")?;
            if arr.len() != block.len() {
                return Err(format!(
                    "译文数量({})与原文({})不一致",
                    arr.len(),
                    block.len()
                ));
            }
            let pairs: Vec<(i64, String)> = block
                .iter()
                .zip(arr.iter())
                .map(|(c, t)| (c.id, t.as_str().unwrap_or("").trim().to_string()))
                .collect();
            Ok::<Vec<(i64, String)>, String>(pairs)
        }
    }))
    .buffered(3);

    let mut translated = 0usize;
    let mut done = 0usize;
    let mut failures: Vec<String> = Vec::new();
    while let Some(res) = stream.next().await {
        done += 1;
        match res {
            Ok(pairs) => {
                let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
                for (cue_id, zh) in &pairs {
                    sqlx::query("UPDATE cues SET text_zh = ? WHERE id = ?")
                        .bind(zh.as_str())
                        .bind(*cue_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                tx.commit().await.map_err(|e| e.to_string())?;
                translated += pairs.len();
                let _ = app.emit(
                    "translate-progress",
                    TranslateProgress {
                        video_id,
                        done,
                        total,
                        pairs,
                    },
                );
            }
            Err(e) => {
                failures.push(e);
                let _ = app.emit(
                    "translate-progress",
                    TranslateProgress {
                        video_id,
                        done,
                        total,
                        pairs: Vec::new(),
                    },
                );
            }
        }
    }

    if !failures.is_empty() {
        return Err(format!(
            "{}/{} 个分块翻译失败（已译 {} 句已保存，重新点击翻译可续传）：{}",
            failures.len(),
            total,
            translated,
            failures[0]
        ));
    }
    Ok(translated)
}

#[tauri::command]
pub async fn generate_tldr(
    pool: tauri::State<'_, SqlitePool>,
    video_id: i64,
) -> Result<String, String> {
    let cfg = LlmConfig::from_settings(&pool)
        .await
        .map_err(|e| e.to_string())?;

    let texts: Vec<(String,)> = sqlx::query_as(
        "SELECT text_en FROM cues WHERE video_id = ? ORDER BY idx",
    )
    .bind(video_id)
    .fetch_all(pool.inner())
    .await
    .map_err(|e| e.to_string())?;
    if texts.is_empty() {
        return Err("还没有字幕，无法生成 TLDR".into());
    }
    let mut full = texts.into_iter().map(|t| t.0).collect::<Vec<_>>().join(" ");
    // 超长截断，控制 token
    if full.chars().count() > 12000 {
        full = full.chars().take(12000).collect();
        full.push_str(" …(后文略)");
    }

    let system = "你是内容摘要助手。根据给出的英文播客字幕文本，用中文写一段 TLDR：\n1. 先用 2-3 句话概括核心内容；\n2. 再用 3-6 个要点列出关键信息；\n3. 语言简洁，直接输出 Markdown 文本，不要前后缀。";
    let tldr = llm::chat(&cfg, system, &full, false)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("UPDATE videos SET tldr = ? WHERE id = ?")
        .bind(&tldr)
        .bind(video_id)
        .execute(pool.inner())
        .await
        .map_err(|e| e.to_string())?;
    Ok(tldr)
}

// ---------- AI 断句分段（阅读区；不含翻译，自动执行，追求最快） ----------

#[derive(Debug, Serialize, Clone)]
pub struct StructureProgress {
    pub video_id: i64,
    pub done: usize,
    pub total: usize,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct CueFull {
    idx: i64,
    start_secs: f64,
    end_secs: f64,
    text_en: String,
}

const STRUCTURE_SYSTEM: &str = r#"你是文本分段助手。输入是按编号给出的播客完整句子。
任务：按话题转换/说话人轮换，把句子聚成段落（2-6 句一段，对话轮替处应分段）。
输出最简 JSON：{"p": [i, ...]} —— 段落断点，值为句子编号（该句结束后另起一段）。
不要输出任何文本内容或解释。"#;

/// 带时间戳的连续词流中的最小单元
#[derive(Debug, Clone)]
struct WordTok {
    text: String,
    start: f64,
    end: f64,
    cue_idx: i64,
}

/// 把 cue 拼成连续词流；cue 内单词按时长均分时间戳
fn build_word_stream(cues: &[CueFull]) -> Vec<WordTok> {
    let mut out = Vec::new();
    for c in cues {
        let words: Vec<&str> = c.text_en.split_whitespace().collect();
        if words.is_empty() {
            continue;
        }
        let dur = (c.end_secs - c.start_secs).max(0.1) / words.len() as f64;
        for (i, w) in words.iter().enumerate() {
            out.push(WordTok {
                text: w.to_string(),
                start: c.start_secs + i as f64 * dur,
                end: c.start_secs + (i + 1) as f64 * dur,
                cue_idx: c.idx,
            });
        }
    }
    out
}

// ---------- 去口水词（整理阶段本地清洗，不调 LLM） ----------

/// 独立成词的思考声/语气词
const FILLER_WORDS: &[&str] = &[
    "um", "umm", "uh", "uhh", "er", "erm", "ah", "eh", "hm", "hmm", "hmmm", "mm", "mmm",
];

fn norm_word(w: &str) -> String {
    w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
}

/// 词流清洗：删思考声、口吃碎片（"wh- what" 式）、连续重复（"I I I" / "sort of sort of"）。
/// 重复判定要求原文完全一致（仅大小写可不同），保留 "very, very" 这类强调重复。
fn clean_word_stream(words: &[WordTok]) -> Vec<WordTok> {
    let mut pass1: Vec<WordTok> = Vec::with_capacity(words.len());
    for (i, w) in words.iter().enumerate() {
        if FILLER_WORDS.contains(&norm_word(&w.text).as_str()) {
            continue;
        }
        if let Some(frag) = w.text.strip_suffix('-') {
            let frag = frag.to_lowercase();
            if !frag.is_empty()
                && words
                    .get(i + 1)
                    .is_some_and(|n| norm_word(&n.text).starts_with(&frag))
            {
                continue;
            }
        }
        pass1.push(w.clone());
    }
    let mut out: Vec<WordTok> = Vec::with_capacity(pass1.len());
    let mut i = 0;
    while i < pass1.len() {
        let mut skipped = false;
        for n in (1..=3).rev() {
            if out.len() >= n && i + n <= pass1.len() {
                let same = (0..n).all(|k| {
                    pass1[i + k]
                        .text
                        .eq_ignore_ascii_case(&out[out.len() - n + k].text)
                });
                if same {
                    i += n;
                    skipped = true;
                    break;
                }
            }
        }
        if !skipped {
            out.push(pass1[i].clone());
            i += 1;
        }
    }
    out
}

/// 插入式口水短语（需逗号佐证才删，避免误删实义用法）
const FILLER_PHRASES: &[&str] = &["you know", "i mean", "you see"];
/// 句首口水开场白
const FILLER_OPENERS: &[&str] = &["well", "so", "you know", "i mean", "you see"];

/// 句级清洗：句首 "Well, / You know, "、句中 ", you know, "、句尾 ", you know."（可链式叠加）
fn clean_sentence_text(text: &str) -> String {
    let mut s = text.trim().to_string();
    // 句首开场白（链式："Well, you know, I think" → "i think"）
    loop {
        let lower = s.to_lowercase();
        let hit = FILLER_OPENERS
            .iter()
            .map(|p| format!("{p},"))
            .find(|pat| lower.starts_with(pat) && lower[pat.len()..].starts_with(' '));
        match hit {
            Some(pat) => s = s[pat.len()..].trim_start().to_string(),
            None => break,
        }
    }
    // 句中：", you know, " → ", "
    for p in FILLER_PHRASES {
        let pat = format!(", {p},");
        while let Some(pos) = s.to_lowercase().find(&pat) {
            s = format!("{},{}", &s[..pos], &s[pos + pat.len()..]);
        }
    }
    // 句尾：", you know." → "."（保留结尾标点）
    for p in FILLER_PHRASES {
        let pat = format!(", {p}");
        if let Some(pos) = s.to_lowercase().rfind(&pat) {
            let after = &s[pos + pat.len()..];
            if !after.is_empty()
                && after
                    .chars()
                    .all(|c| matches!(c, '.' | '!' | '?' | '"' | '\'' | ')'))
            {
                s = format!("{}{}", &s[..pos], after);
            }
        }
    }
    // 句首被剥掉后重新大写
    if s.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
        let first = s.chars().next().unwrap().to_ascii_uppercase();
        s.replace_range(..1, &first.to_string());
    }
    s
}

const ABBREVIATIONS: &[&str] = &[
    "mr", "mrs", "ms", "dr", "st", "vs", "etc", "e.g", "i.e", "no", "fig", "approx", "u.s",
    "u.k", "u.n", "a.m", "p.m", "jr", "sr", "prof", "inc", "ltd", "co", "corp", "dept",
];

/// 该单词是否终结一个句子（保留标点附着）
fn is_sentence_end(word: &str) -> bool {
    let trimmed = word.trim_end_matches(['"', '\'', ')', ']', '}']);
    let last = trimmed.chars().last();
    if !matches!(last, Some('.') | Some('!') | Some('?')) {
        return false;
    }
    // 数字中的小数点不算
    if trimmed
        .trim_end_matches(['.', '!', '?'])
        .chars()
        .last()
        .is_some_and(|c| c.is_ascii_digit())
        && trimmed.contains('.')
        && !trimmed.ends_with(['!', '?'])
    {
        return false;
    }
    // 常见缩写不算
    let base = trimmed.trim_end_matches('.').to_lowercase();
    if ABBREVIATIONS.contains(&base.as_str()) {
        return false;
    }
    true
}

/// 词流 → 完整句子（文本、时间、cue 区间）。本地规则，瞬时且边界精确。
fn split_sentences_from_stream(words: &[WordTok]) -> Vec<(String, f64, f64, i64, i64)> {
    let mut out = Vec::new();
    let mut buf: Vec<&WordTok> = Vec::new();
    for w in words {
        buf.push(w);
        if is_sentence_end(&w.text) {
            let text = buf.iter().map(|t| t.text.as_str()).collect::<Vec<_>>().join(" ");
            out.push((
                text,
                buf[0].start,
                buf[buf.len() - 1].end,
                buf[0].cue_idx,
                buf[buf.len() - 1].cue_idx,
            ));
            buf.clear();
        }
    }
    if !buf.is_empty() {
        let text = buf.iter().map(|t| t.text.as_str()).collect::<Vec<_>>().join(" ");
        out.push((
            text,
            buf[0].start,
            buf[buf.len() - 1].end,
            buf[0].cue_idx,
            buf[buf.len() - 1].cue_idx,
        ));
    }
    out
}

/// 段落分组的本地兜底：时间间隔 >2s 即分段。
fn local_paragraph_breaks(sents: &[(String, f64, f64, i64, i64)]) -> Vec<usize> {
    let mut breaks = Vec::new();
    for i in 0..sents.len().saturating_sub(1) {
        if sents[i + 1].1 - sents[i].2 > 2.0 {
            breaks.push(i);
        }
    }
    breaks
}

/// AI 断句分段：
/// 1) cue → 连续词流，本地去口水词（思考声/口吃碎片/连续重复/插入语）后按标点切出完整句子（瞬时、边界精确到词）
/// 2) LLM 只做段落分组（输入完整句子、只回断点编号，输出极小），失败回退时间间隔规则
#[tauri::command]
pub async fn structure_video(
    pool: tauri::State<'_, SqlitePool>,
    app: AppHandle,
    video_id: i64,
    force: Option<bool>,
) -> Result<usize, String> {
    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sentences WHERE video_id = ?")
        .bind(video_id)
        .fetch_one(pool.inner())
        .await
        .map_err(|e| e.to_string())?;
    if existing > 0 && !force.unwrap_or(false) {
        return Ok(existing as usize);
    }

    let cues = sqlx::query_as::<_, CueFull>(
        "SELECT idx, start_secs, end_secs, text_en FROM cues WHERE video_id = ? ORDER BY idx",
    )
    .bind(video_id)
    .fetch_all(pool.inner())
    .await
    .map_err(|e| e.to_string())?;
    if cues.is_empty() {
        return Err("还没有字幕，无法分段".into());
    }

    // 第一步（本地、瞬时）：连续词流 → 去口水词 → 完整句子
    let words = build_word_stream(&cues);
    let words = clean_word_stream(&words);
    let sents: Vec<(String, f64, f64, i64, i64)> = split_sentences_from_stream(&words)
        .into_iter()
        .map(|(t, st, en, cf, ct)| (clean_sentence_text(&t), st, en, cf, ct))
        .filter(|(t, ..)| !t.is_empty())
        .collect();
    if sents.is_empty() {
        return Err("未能切分出句子".into());
    }

    // 第二步（LLM，只回段落断点）：按 150 句一块、3 路并发
    let cfg = LlmConfig::from_settings(&pool)
        .await
        .map_err(|e| e.to_string())?;
    const CHUNK: usize = 150;
    const CONCURRENCY: usize = 3;
    let chunks: Vec<Vec<(usize, String)>> = sents
        .iter()
        .enumerate()
        .map(|(i, s)| (i, s.0.clone()))
        .collect::<Vec<_>>()
        .chunks(CHUNK)
        .map(|c| c.to_vec())
        .collect();
    let total = chunks.len();
    let done = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let chunk_results: Vec<Vec<usize>> = futures::stream::iter(chunks.into_iter())
        .map(|chunk| {
            let cfg = cfg.clone();
            let app = app.clone();
            let done = done.clone();
            async move {
                let numbered = chunk
                    .iter()
                    .map(|(i, t)| format!("{}. {}", i, t))
                    .collect::<Vec<_>>()
                    .join("\n");
                let breaks = match llm::chat(&cfg, STRUCTURE_SYSTEM, &numbered, true).await {
                    Ok(out) => {
                        let json_text = out
                            .find('{')
                            .and_then(|s| out.rfind('}').map(|e| &out[s..=e]))
                            .unwrap_or(&out);
                        serde_json::from_str::<serde_json::Value>(json_text)
                            .ok()
                            .and_then(|parsed| parsed.get("p")?.as_array().cloned())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_u64().map(|n| n as usize))
                                    .collect::<Vec<_>>()
                            })
                    }
                    Err(_) => None,
                };
                let d = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                let _ = app.emit(
                    "structure-progress",
                    StructureProgress { video_id, done: d, total },
                );
                breaks.unwrap_or_default()
            }
        })
        .buffered(CONCURRENCY)
        .collect()
        .await;

    let mut para_breaks: Vec<usize> = chunk_results.into_iter().flatten().collect();
    para_breaks.sort_unstable();
    para_breaks.dedup();
    if para_breaks.is_empty() {
        para_breaks = local_paragraph_breaks(&sents);
    }

    // 汇总写库（中文留空，翻译后由 fill_sentence_zh 本地补齐）
    let mut rows: Vec<(usize, usize, f64, f64, String, i64, i64)> = Vec::new();
    let mut para = 0usize;
    let mut sent_in_para = 0usize;
    for (i, (text, start, end, cue_from, cue_to)) in sents.iter().enumerate() {
        rows.push((para, sent_in_para, *start, *end, text.clone(), *cue_from, *cue_to));
        sent_in_para += 1;
        if para_breaks.contains(&i) {
            para += 1;
            sent_in_para = 0;
        }
    }

    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM sentences WHERE video_id = ?")
        .bind(video_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    for (p, si, st, en, text_en, cue_from, cue_to) in &rows {
        sqlx::query(
            "INSERT INTO sentences (video_id, para_idx, sent_idx, start_secs, end_secs, text_en, cue_from, cue_to) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(video_id)
        .bind(*p as i64)
        .bind(*si as i64)
        .bind(st)
        .bind(en)
        .bind(text_en)
        .bind(cue_from)
        .bind(cue_to)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(rows.len())
}

/// 翻译完成后调用：按句子的 cue 区间本地拼接句级中文，**不调用 LLM**（即时完成）。
#[tauri::command]
pub async fn fill_sentence_zh(
    pool: tauri::State<'_, SqlitePool>,
    video_id: i64,
) -> Result<usize, String> {
    let sents = sqlx::query_as::<_, SentSpan>(
        "SELECT id, start_secs, end_secs, cue_from, cue_to FROM sentences WHERE video_id = ? AND text_zh IS NULL ORDER BY start_secs",
    )
    .bind(video_id)
    .fetch_all(pool.inner())
    .await
    .map_err(|e| e.to_string())?;
    if sents.is_empty() {
        return Ok(0);
    }
    let cues = sqlx::query_as::<_, CueZh>(
        "SELECT idx, start_secs, end_secs, text_zh FROM cues WHERE video_id = ? AND text_zh IS NOT NULL ORDER BY idx",
    )
    .bind(video_id)
    .fetch_all(pool.inner())
    .await
    .map_err(|e| e.to_string())?;

    let assigned = assign_sentence_zh(&sents, &cues);
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    for (sent_id, zh) in &assigned {
        sqlx::query("UPDATE sentences SET text_zh = ? WHERE id = ?")
            .bind(zh)
            .bind(*sent_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(assigned.len())
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct SentSpan {
    id: i64,
    start_secs: f64,
    end_secs: f64,
    cue_from: i64,
    cue_to: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct CueZh {
    idx: i64,
    start_secs: f64,
    end_secs: f64,
    text_zh: String,
}

/// 句级中文划分：句子边界常落在 cue 内部，相邻句子共享边界 cue，
/// 整段拼接会导致译文重复/错配。按时间重叠比例划分共享 cue 的译文，
/// 用「每 cue 已消费游标」保证相邻句子互补（不重复、不遗漏）；
/// 切分点就近顺延到句末强标点；覆盖 cue 尾部的句子拿走剩余全部。
fn assign_sentence_zh(sents: &[SentSpan], cues: &[CueZh]) -> Vec<(i64, String)> {
    let mut consumed: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
    let mut out = Vec::new();
    for s in sents {
        let mut zh = String::new();
        for c in cues
            .iter()
            .filter(|c| c.idx >= s.cue_from && c.idx <= s.cue_to)
        {
            let total = c.text_zh.chars().count();
            let cur = consumed.get(&c.idx).copied().unwrap_or(0);
            if cur >= total {
                continue;
            }
            let cut = if s.end_secs >= c.end_secs - 0.01 {
                total
            } else {
                let dur = (c.end_secs - c.start_secs).max(0.01);
                let overlap =
                    (s.end_secs.min(c.end_secs) - s.start_secs.max(c.start_secs)).max(0.0);
                let fair = cur + ((total as f64) * (overlap / dur)).round() as usize;
                snap_zh_boundary(&c.text_zh, fair, cur, total)
            };
            zh.extend(c.text_zh.chars().skip(cur).take(cut - cur));
            consumed.insert(c.idx, cut);
        }
        let zh = zh.trim().to_string();
        if !zh.is_empty() {
            out.push((s.id, zh));
        }
    }
    out
}

/// 把切分点就近对齐到句末强标点之后（先向后找 2 字，再向前找 8 字），找不到保持原切分点
fn snap_zh_boundary(zh: &str, cut: usize, min: usize, total: usize) -> usize {
    let base = cut.clamp(min, total);
    if base == min || base == total {
        return base;
    }
    let chars: Vec<char> = zh.chars().collect();
    let strong = |c: char| matches!(c, '。' | '！' | '？' | '!' | '?' | '；' | ';');
    let back = base.saturating_sub(2).max(min).max(1);
    for p in (back..=base).rev() {
        if strong(chars[p - 1]) {
            return p;
        }
    }
    let limit = (base + 8).min(total);
    for p in base + 1..=limit {
        if strong(chars[p - 1]) {
            return p;
        }
    }
    base
}

#[tauri::command]
pub async fn list_sentences(
    pool: tauri::State<'_, SqlitePool>,
    video_id: i64,
) -> Result<Vec<crate::models::Sentence>, String> {
    sqlx::query_as::<_, crate::models::Sentence>(
        "SELECT * FROM sentences WHERE video_id = ? ORDER BY para_idx, sent_idx",
    )
    .bind(video_id)
    .fetch_all(pool.inner())
    .await
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(idx: i64, start: f64, end: f64, text: &str) -> CueFull {
        CueFull {
            idx,
            start_secs: start,
            end_secs: end,
            text_en: text.to_string(),
        }
    }

    #[test]
    fn word_stream_splits_complete_sentences() {
        // 用户的真实例子：cue 边界 ≠ 句子边界
        let cues = vec![
            cue(0, 0.0, 2.0, "these games have to end. I think that"),
            cue(1, 2.0, 5.0, "they have to choose between either renouncing this quote"),
            cue(2, 5.0, 7.0, "whistleblower as a doomer op. He's basically an entry-level"),
            cue(3, 7.0, 10.0, "no nothing who's engaging in hyperbole and science fiction. It's all vibes. In"),
            cue(4, 10.0, 13.0, "other words, there's not facts and evidence behind this, right?"),
        ];
        let stream = build_word_stream(&cues);
        let sents = split_sentences_from_stream(&stream);
        assert_eq!(sents.len(), 5);
        assert_eq!(sents[0].0, "these games have to end.");
        assert_eq!(
            sents[1].0,
            "I think that they have to choose between either renouncing this quote whistleblower as a doomer op."
        );
        assert_eq!(
            sents[2].0,
            "He's basically an entry-level no nothing who's engaging in hyperbole and science fiction."
        );
        assert_eq!(sents[3].0, "It's all vibes.");
        assert!(sents[4].0.starts_with("In other words,"));
        // 时间戳取自词流：第 2 句从 "I"（cue 0 内）开始
        assert!(sents[1].1 > 0.0 && sents[1].1 < 2.0);
        // cue 区间记录正确
        assert_eq!((sents[1].3, sents[1].4), (0, 2));
    }

    #[test]
    fn abbreviations_and_decimals_not_split() {
        let cues = vec![cue(0, 0.0, 4.0, "Mr. Smith paid 3.5 dollars. Done")];
        let stream = build_word_stream(&cues);
        let sents = split_sentences_from_stream(&stream);
        assert_eq!(sents.len(), 2);
        assert_eq!(sents[0].0, "Mr. Smith paid 3.5 dollars.");
    }

    fn structure_sents(text: &str) -> Vec<String> {
        let cues = vec![cue(0, 0.0, 10.0, text)];
        let words = clean_word_stream(&build_word_stream(&cues));
        split_sentences_from_stream(&words)
            .into_iter()
            .map(|(t, ..)| clean_sentence_text(&t))
            .filter(|t| !t.is_empty())
            .collect()
    }

    #[test]
    fn filler_words_and_repeats_removed() {
        let sents = structure_sents("um I I I think that, uh, they should go.");
        assert_eq!(sents, vec!["I think that, they should go."]);
    }

    #[test]
    fn stutter_fragments_removed() {
        let sents = structure_sents("I wh- what is this?");
        assert_eq!(sents, vec!["I what is this?"]);
    }

    #[test]
    fn filler_phrases_need_commas() {
        // 有逗号佐证的插入语才删除
        let sents = structure_sents("It was, you know, difficult. I will let you know.");
        assert_eq!(sents, vec!["It was, difficult.", "I will let you know."]);
    }

    #[test]
    fn openers_and_trailing_phrases_removed() {
        let sents = structure_sents("Well, you know, that's it, I mean.");
        assert_eq!(sents, vec!["That's it."]);
    }

    #[test]
    fn emphasis_repetition_kept() {
        // 带标点的强调重复不是口吃，保留
        let sents = structure_sents("It was very, very good. No, no way.");
        assert_eq!(sents, vec!["It was very, very good.", "No, no way."]);
    }

    #[test]
    fn multiword_repeats_removed() {
        let sents = structure_sents("I sort of sort of wanted to go.");
        assert_eq!(sents, vec!["I sort of wanted to go."]);
    }

    fn sent(id: i64, start: f64, end: f64, cf: i64, ct: i64) -> SentSpan {
        SentSpan {
            id,
            start_secs: start,
            end_secs: end,
            cue_from: cf,
            cue_to: ct,
        }
    }

    fn cuez(idx: i64, start: f64, end: f64, zh: &str) -> CueZh {
        CueZh {
            idx,
            start_secs: start,
            end_secs: end,
            text_zh: zh.to_string(),
        }
    }

    #[test]
    fn shared_cue_zh_partitioned_without_duplication() {
        // 句子边界落在 cue 内部：前后两句共享同一个 cue，译文必须互补划分
        let sents = vec![sent(1, 0.0, 2.0, 0, 0), sent(2, 2.0, 4.0, 0, 0)];
        let cues = vec![cuez(0, 0.0, 4.0, "第一句话。第二句话。")];
        let out = assign_sentence_zh(&sents, &cues);
        assert_eq!(out[0].1, "第一句话。");
        assert_eq!(out[1].1, "第二句话。");
    }

    #[test]
    fn three_sentences_sharing_one_cue() {
        let sents = vec![
            sent(1, 0.0, 3.0, 0, 0),
            sent(2, 3.0, 6.0, 0, 0),
            sent(3, 6.0, 10.0, 0, 0),
        ];
        let cues = vec![cuez(0, 0.0, 10.0, "第一句。第二句。第三句。")];
        let out = assign_sentence_zh(&sents, &cues);
        assert_eq!(out[0].1, "第一句。");
        assert_eq!(out[1].1, "第二句。");
        assert_eq!(out[2].1, "第三句。");
    }

    #[test]
    fn partition_without_punctuation_still_complementary() {
        let sents = vec![sent(1, 0.0, 2.0, 0, 0), sent(2, 2.0, 4.0, 0, 0)];
        let cues = vec![cuez(0, 0.0, 4.0, "abcdefgh")];
        let out = assign_sentence_zh(&sents, &cues);
        assert_eq!(out[0].1, "abcd");
        assert_eq!(out[1].1, "efgh");
    }

    #[test]
    fn cue_fully_inside_sentence_takes_all() {
        let sents = vec![sent(1, 0.0, 10.0, 0, 2)];
        let cues = vec![
            cuez(0, 0.0, 2.0, "甲。"),
            cuez(1, 2.0, 5.0, "乙。"),
            cuez(2, 5.0, 10.0, "丙。"),
        ];
        let out = assign_sentence_zh(&sents, &cues);
        assert_eq!(out[0].1, "甲。乙。丙。");
    }
}
