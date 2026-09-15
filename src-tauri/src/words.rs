//! 生词卡片：ECDict 查询命令、AI 上下文解析、卡片 CRUD。

use crate::dict::{self, DictEntry};
use crate::llm::{self, LlmConfig};
use crate::models::WordCard;
use sqlx::SqlitePool;

/// 从字幕中提取包含目标词的完整句子作为上下文：
/// 以当前 cue 为中心向前后各扩展 2 条，拼接后按句切分，取包含该词的那句。
async fn extract_context(
    pool: &SqlitePool,
    video_id: i64,
    cue_id: i64,
    word: &str,
) -> Option<String> {
    let idx: Option<(i64,)> =
        sqlx::query_as("SELECT idx FROM cues WHERE id = ? AND video_id = ?")
            .bind(cue_id)
            .bind(video_id)
            .fetch_optional(pool)
            .await
            .ok()?;
    let idx = idx?.0;
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT text_en FROM cues WHERE video_id = ? AND idx BETWEEN ? AND ? ORDER BY idx",
    )
    .bind(video_id)
    .bind(idx - 2)
    .bind(idx + 2)
    .fetch_all(pool)
    .await
    .ok()?;
    let text = rows
        .into_iter()
        .map(|r| r.0)
        .collect::<Vec<_>>()
        .join(" ");

    let sentences = split_sentences(&text);
    let word_lower = word.to_lowercase();
    // 优先整词匹配所在句
    sentences
        .iter()
        .find(|s| contains_word(s, &word_lower))
        .or_else(|| sentences.iter().find(|s| s.to_lowercase().contains(&word_lower)))
        .map(|s| s.to_string())
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        cur.push(c);
        if matches!(c, '.' | '!' | '?') {
            out.push(cur.trim().to_string());
            cur.clear();
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// 大小写不敏感的整词匹配。
fn contains_word(sentence: &str, word_lower: &str) -> bool {
    sentence
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .any(|w| w.to_lowercase() == word_lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_sentence_containing_word() {
        let text = "The scop cance canceled on us. Is that what happened? Yeah, the scop cance canceled.";
        let sentences = split_sentences(text);
        assert_eq!(sentences.len(), 3);
        assert_eq!(
            sentences
                .iter()
                .find(|s| contains_word(s, "happened"))
                .unwrap(),
            "Is that what happened?"
        );
        assert!(contains_word("Yeah, the scop cance canceled.", "canceled"));
        assert!(!contains_word("Yeah, the scop cance canceled.", "cancel"));
    }
}

async fn dict_path(pool: &SqlitePool) -> Result<String, String> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'dict_path'")
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    row.map(|r| r.0)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "请先在设置页配置 ECDICT 词典文件路径".to_string())
}

#[derive(Debug, serde::Serialize)]
pub struct RichLookup {
    /// MDX 词典的富文本释义（HTML，含例句）；无 MDX 或查不到为 None
    pub mdx_html: Option<String>,
    /// ECDICT 基础词条（音标/词形/标签），MDX 命中时也一并返回用于音标等
    pub entry: Option<DictEntry>,
}

/// 发音：macOS 系统 TTS（离线兜底）。固定英文语音，避免系统默认中文语音按中文规则拼读英文。
#[tauri::command]
pub fn speak_word(word: String) {
    let word = word.trim().to_string();
    if word.is_empty() || word.len() > 100 {
        return;
    }
    std::thread::spawn(move || {
        // Samantha 为 macOS 自带美式英文语音；缺失时依次回退，最后退回默认语音
        for voice in ["Samantha", "Alex", "Daniel"] {
            if let Ok(out) = std::process::Command::new("say")
                .args(["-v", voice, &word])
                .output()
            {
                if out.status.success() {
                    return;
                }
            }
        }
        let _ = std::process::Command::new("say").arg(&word).spawn();
    });
}

#[tauri::command]
pub async fn lookup_word(
    pool: tauri::State<'_, SqlitePool>,
    word: String,
) -> Result<RichLookup, String> {
    let cleaned = word
        .trim()
        .trim_matches(|c: char| !c.is_alphabetic() && c != '\'' && c != '-')
        .to_string();
    if cleaned.is_empty() {
        return Ok(RichLookup { mdx_html: None, entry: None });
    }
    // 1) MDX 富文本词典（如已导入）
    let mdx_html = crate::mdx::lookup_mdx(&pool, &cleaned).await?;
    // 2) ECDICT（音标/标签/简明释义；MDX 未命中时作主释义）
    let entry = match dict_path(&pool).await {
        Ok(path) => dict::lookup(&path, &cleaned).await?,
        Err(_) => None,
    };
    if mdx_html.is_none() && entry.is_none() && dict_path(&pool).await.is_err() {
        return Err("未找到词条。可在设置页配置 ECDICT 或导入 MDX 词典".into());
    }
    Ok(RichLookup { mdx_html, entry })
}

#[tauri::command]
pub async fn analyze_word(
    pool: tauri::State<'_, SqlitePool>,
    word: String,
    context: String,
    video_id: Option<i64>,
    cue_id: Option<i64>,
) -> Result<String, String> {
    let cfg = LlmConfig::from_settings(&pool)
        .await
        .map_err(|e| e.to_string())?;
    // 同样优先提取完整句子，解析更准确
    let context = match (video_id, cue_id) {
        (Some(vid), Some(cid)) => extract_context(&pool, vid, cid, &word)
            .await
            .unwrap_or(context),
        _ => context,
    };
    let system = "你是英语词汇老师。用户给你一个单词和它在播客字幕中的原句，请用中文简明解析：\n1. 该词在此句中的具体含义和词性；\n2. 常见搭配或用法（如有）；\n3. 如有熟词僻义请特别指出。\n控制在 120 字以内，直接输出解析文本。";
    let user = format!("单词：{word}\n原句：{context}");
    llm::chat(&cfg, system, &user, false)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_word_card(
    pool: tauri::State<'_, SqlitePool>,
    word: String,
    phonetic: Option<String>,
    definition: Option<String>,
    ai_analysis: Option<String>,
    video_id: Option<i64>,
    cue_id: Option<i64>,
    context: Option<String>,
    start_secs: Option<f64>,
) -> Result<i64, String> {
    // 上下文优先取「包含该词的完整句子」；退化用前端传入的 cue 文本
    let context = match (video_id, cue_id) {
        (Some(vid), Some(cid)) => extract_context(&pool, vid, cid, &word)
            .await
            .or(context),
        _ => context,
    };
    let id = sqlx::query(
        "INSERT INTO word_cards (word, phonetic, definition, ai_analysis, video_id, cue_id, context, start_secs)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(word, video_id, cue_id) DO UPDATE SET
           phonetic = excluded.phonetic,
           definition = excluded.definition,
           ai_analysis = COALESCE(excluded.ai_analysis, word_cards.ai_analysis),
           context = excluded.context,
           start_secs = excluded.start_secs",
    )
    .bind(&word)
    .bind(phonetic)
    .bind(definition)
    .bind(ai_analysis)
    .bind(video_id)
    .bind(cue_id)
    .bind(context)
    .bind(start_secs)
    .execute(pool.inner())
    .await
    .map_err(|e| e.to_string())?
    .last_insert_rowid();
    Ok(id)
}

/// 该视频已收藏的生词（用于字幕内高亮 + 悬停释义）
#[tauri::command]
pub async fn list_saved_words(
    pool: tauri::State<'_, SqlitePool>,
    video_id: i64,
) -> Result<Vec<SavedWord>, String> {
    sqlx::query_as::<_, SavedWord>(
        "SELECT DISTINCT word, definition, ai_analysis FROM word_cards WHERE video_id = ?",
    )
    .bind(video_id)
    .fetch_all(pool.inner())
    .await
    .map_err(|e| e.to_string())
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct SavedWord {
    pub word: String,
    pub definition: Option<String>,
    pub ai_analysis: Option<String>,
}

#[tauri::command]
pub async fn list_word_cards(
    pool: tauri::State<'_, SqlitePool>,
) -> Result<Vec<WordCard>, String> {
    sqlx::query_as::<_, WordCard>(
        "SELECT w.*, v.title AS video_title, v.thumbnail_path AS video_thumb FROM word_cards w LEFT JOIN videos v ON v.id = w.video_id ORDER BY w.created_at DESC",
    )
    .fetch_all(pool.inner())
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_word_card(
    pool: tauri::State<'_, SqlitePool>,
    id: i64,
) -> Result<(), String> {
    sqlx::query("DELETE FROM word_cards WHERE id = ?")
        .bind(id)
        .execute(pool.inner())
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ---------- 复习系统（类 Duolingo 记忆曲线） ----------

/// 待复习生词：未掌握优先，按失败连击数倒序、最久未复习优先，随机取 limit 个
#[tauri::command]
pub async fn list_review_candidates(
    pool: tauri::State<'_, SqlitePool>,
    limit: i64,
) -> Result<Vec<WordCard>, String> {
    let mut rows = sqlx::query_as::<_, WordCard>(
        "SELECT w.*, v.title AS video_title, v.thumbnail_path AS video_thumb
         FROM word_cards w LEFT JOIN videos v ON v.id = w.video_id
         WHERE w.mastered = 0
         ORDER BY w.fail_streak DESC, w.last_reviewed_at IS NOT NULL, w.last_reviewed_at ASC
         LIMIT ?",
    )
    .bind(limit * 3)
    .fetch_all(pool.inner())
    .await
    .map_err(|e| e.to_string())?;
    // 简单洗牌取前 limit
    use rand::seq::SliceRandom;
    rows.shuffle(&mut rand::rng());
    rows.truncate(limit as usize);
    Ok(rows)
}

/// 词义选择题干扰项：其他生词的释义
#[tauri::command]
pub async fn random_distractors(
    pool: tauri::State<'_, SqlitePool>,
    exclude_word: String,
    count: i64,
) -> Result<Vec<String>, String> {
    sqlx::query_as::<_, (String,)>(
        "SELECT DISTINCT definition FROM word_cards WHERE word != ? AND definition IS NOT NULL AND definition != '' ORDER BY RANDOM() LIMIT ?",
    )
    .bind(exclude_word)
    .bind(count)
    .fetch_all(pool.inner())
    .await
    .map(|rows| rows.into_iter().map(|r| r.0).collect())
    .map_err(|e| e.to_string())
}

/// 提交一次作答：correct=首次答对, hinted=提示后答对, failed=答错, skipped=跳过
/// 规则：失败连击 ≥3 判定未掌握；累计 3 次成功（review_count≥3 且无连击）判定掌握
#[tauri::command]
pub async fn submit_review(
    pool: tauri::State<'_, SqlitePool>,
    id: i64,
    result: String,
) -> Result<(), String> {
    match result.as_str() {
        "correct" | "hinted" => {
            sqlx::query(
                "UPDATE word_cards SET
                   review_count = review_count + 1,
                   fail_streak = 0,
                   mastered = CASE WHEN review_count + 1 >= 3 THEN 1 ELSE 0 END,
                   last_reviewed_at = datetime('now')
                 WHERE id = ?",
            )
        }
        "failed" | "skipped" => {
            sqlx::query(
                "UPDATE word_cards SET
                   fail_streak = fail_streak + 1,
                   mastered = 0,
                   last_reviewed_at = datetime('now')
                 WHERE id = ?",
            )
        }
        _ => return Err("未知的作答结果".into()),
    }
    .bind(id)
    .execute(pool.inner())
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct ReviewRound {
    pub id: i64,
    pub total: i64,
    pub correct: i64,
    pub score: i64,
    pub created_at: String,
}

#[tauri::command]
pub async fn save_review_round(
    pool: tauri::State<'_, SqlitePool>,
    total: i64,
    correct: i64,
    score: i64,
) -> Result<i64, String> {
    let id = sqlx::query("INSERT INTO review_rounds (total, correct, score) VALUES (?, ?, ?)")
        .bind(total)
        .bind(correct)
        .bind(score)
        .execute(pool.inner())
        .await
        .map_err(|e| e.to_string())?
        .last_insert_rowid();
    Ok(id)
}

#[tauri::command]
pub async fn list_review_rounds(
    pool: tauri::State<'_, SqlitePool>,
    limit: i64,
) -> Result<Vec<ReviewRound>, String> {
    sqlx::query_as::<_, ReviewRound>(
        "SELECT * FROM review_rounds ORDER BY created_at DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool.inner())
    .await
    .map_err(|e| e.to_string())
}

#[derive(Debug, serde::Serialize)]
pub struct ReviewStats {
    pub total: i64,
    pub mastered: i64,
    pub due: i64,
}

#[tauri::command]
pub async fn review_stats(pool: tauri::State<'_, SqlitePool>) -> Result<ReviewStats, String> {
    let (total,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM word_cards")
        .fetch_one(pool.inner())
        .await
        .map_err(|e| e.to_string())?;
    let (mastered,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM word_cards WHERE mastered = 1")
            .fetch_one(pool.inner())
            .await
            .map_err(|e| e.to_string())?;
    Ok(ReviewStats {
        total,
        mastered,
        due: total - mastered,
    })
}
