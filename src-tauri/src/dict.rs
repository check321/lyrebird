//! ECDICT 本地词典查询（stardict.sqlite）。

use serde::Serialize;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

#[derive(Debug, Serialize, FromRow)]
pub struct DictEntry {
    pub word: String,
    pub phonetic: Option<String>,
    pub definition: Option<String>,
    pub translation: Option<String>,
    pub pos: Option<String>,
    pub tag: Option<String>,
    pub exchange: Option<String>,
    pub collins: Option<i64>,
    pub bnc: Option<i64>,
    pub frq: Option<i64>,
}

/// 查询 ECDICT。先精确匹配，再尝试小写；查不到返回 None。
pub async fn lookup(dict_path: &str, word: &str) -> Result<Option<DictEntry>, String> {
    let options = SqliteConnectOptions::from_str(&format!("sqlite://{dict_path}"))
        .map_err(|e| e.to_string())?
        .read_only(true);
    let pool = SqlitePool::connect_with(options)
        .await
        .map_err(|e| format!("无法打开词典文件: {e}"))?;

    let mut candidates = vec![word.to_string()];
    let lower = word.to_lowercase();
    if lower != word {
        candidates.push(lower);
    }

    for w in &candidates {
        let entry = sqlx::query_as::<_, DictEntry>(
            "SELECT word, phonetic, definition, translation, pos, tag, exchange, collins, bnc, frq FROM stardict WHERE word = ? LIMIT 1",
        )
        .bind(w)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?;
        if entry.is_some() {
            pool.close().await;
            return Ok(entry);
        }
    }
    pool.close().await;
    Ok(None)
}
