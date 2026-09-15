-- MDX 词典索引：从 .mdx 导入的富文本词条（HTML，含例句）
CREATE TABLE IF NOT EXISTS mdx_entries (
    key        TEXT NOT NULL,
    html       TEXT NOT NULL,
    source     TEXT NOT NULL,
    PRIMARY KEY (key, source)
);

CREATE INDEX IF NOT EXISTS idx_mdx_entries_key ON mdx_entries (key COLLATE NOCASE);

-- 记录已索引的 mdx 文件路径
CREATE TABLE IF NOT EXISTS mdx_sources (
    path       TEXT PRIMARY KEY,
    title      TEXT,
    entries    INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
