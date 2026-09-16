CREATE TABLE IF NOT EXISTS subscriptions (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    channel_id     TEXT NOT NULL UNIQUE,
    title          TEXT NOT NULL,
    url            TEXT NOT NULL,
    avatar         TEXT,
    follower_count INTEGER,
    created_at     TEXT NOT NULL DEFAULT (datetime('now'))
);
