//! 用户配置项：SQLite settings 表读写，key 清单与默认值由前端定义。

use sqlx::SqlitePool;
use std::collections::HashMap;

#[tauri::command]
pub async fn get_settings(
    pool: tauri::State<'_, SqlitePool>,
) -> Result<HashMap<String, String>, String> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM settings")
        .fetch_all(pool.inner())
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().collect())
}

#[tauri::command]
pub async fn save_settings(
    pool: tauri::State<'_, SqlitePool>,
    values: HashMap<String, String>,
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    for (k, v) in values {
        sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
            .bind(k)
            .bind(v)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())
}
