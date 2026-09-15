//! MDX 词典（欧路词典同源格式）：导入索引 + 富文本查询。

use mdict_reader::{MdictReader, Mdx, RecordData};
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, serde::Serialize)]
pub struct MdxImportProgress {
    pub stage: String,
    pub entries: usize,
}

/// 导入 .mdx 词典：全部词条写入 mdx_entries 索引表（同路径重导会先清空旧数据）。
#[tauri::command]
pub async fn import_mdx(
    pool: tauri::State<'_, SqlitePool>,
    app: AppHandle,
    path: String,
) -> Result<usize, String> {
    let path_c = path.clone();
    let pool_c = pool.inner().clone();
    let app_c = app.clone();
    // mdx 解析是 CPU/IO 密集，放阻塞线程
    let count = tokio::task::spawn_blocking(move || -> Result<usize, String> {
        let reader = MdictReader::<Mdx>::new(&path_c, None, None, true)
            .map_err(|e| format!("无法打开 MDX 文件: {e}"))?;
        let title = reader.metadata().title.clone();

        let rt = tokio::runtime::Handle::current();
        rt.block_on(async {
            sqlx::query("DELETE FROM mdx_entries WHERE source = ?")
                .bind(&path_c)
                .execute(&pool_c)
                .await
                .map_err(|e| e.to_string())?;

            let mut count = 0usize;
            let mut tx = pool_c.begin().await.map_err(|e| e.to_string())?;
            let mut redirects: Vec<(String, String)> = Vec::new();
            for item in reader.iter_records() {
                let (key, record) = item.map_err(|e| e.to_string())?;
                match record {
                    RecordData::Content(html) => {
                        sqlx::query(
                            "INSERT OR REPLACE INTO mdx_entries (key, html, source) VALUES (?, ?, ?)",
                        )
                        .bind(&key)
                        .bind(&html)
                        .bind(&path_c)
                        .execute(&mut *tx)
                        .await
                        .map_err(|e| e.to_string())?;
                        count += 1;
                    }
                    RecordData::Redirect(target) => redirects.push((key, target)),
                }
                if count % 5000 == 0 && count > 0 {
                    tx.commit().await.map_err(|e| e.to_string())?;
                    tx = pool_c.begin().await.map_err(|e| e.to_string())?;
                    let _ = app_c.emit(
                        "mdx-import-progress",
                        MdxImportProgress {
                            stage: "导入词条…".into(),
                            entries: count,
                        },
                    );
                }
            }
            tx.commit().await.map_err(|e| e.to_string())?;

            // 别名/重定向词条（如 goes → go）：复制目标词条 HTML
            let mut tx = pool_c.begin().await.map_err(|e| e.to_string())?;
            for (key, target) in redirects {
                sqlx::query(
                    "INSERT OR REPLACE INTO mdx_entries (key, html, source)
                     SELECT ?, html, ? FROM mdx_entries WHERE key = ? AND source = ?",
                )
                .bind(&key)
                .bind(&path_c)
                .bind(&target)
                .bind(&path_c)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            }
            tx.commit().await.map_err(|e| e.to_string())?;

            sqlx::query(
                "INSERT OR REPLACE INTO mdx_sources (path, title, entries) VALUES (?, ?, ?)",
            )
            .bind(&path_c)
            .bind(&title)
            .bind(count as i64)
            .execute(&pool_c)
            .await
            .map_err(|e| e.to_string())?;

            let _ = app_c.emit(
                "mdx-import-progress",
                MdxImportProgress {
                    stage: "完成".into(),
                    entries: count,
                },
            );
            Ok(count)
        })
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(count)
}

/// MDX 富文本查询（大小写不敏感）。
pub async fn lookup_mdx(pool: &SqlitePool, word: &str) -> Result<Option<String>, String> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT html FROM mdx_entries WHERE key = ? COLLATE NOCASE LIMIT 1",
    )
    .bind(word)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(row.map(|r| r.0))
}

/// 已导入的 MDX 词典列表
#[tauri::command]
pub async fn list_mdx_sources(
    pool: tauri::State<'_, SqlitePool>,
) -> Result<Vec<(String, Option<String>, i64)>, String> {
    sqlx::query_as("SELECT path, title, entries FROM mdx_sources ORDER BY created_at")
        .fetch_all(pool.inner())
        .await
        .map_err(|e| e.to_string())
}
