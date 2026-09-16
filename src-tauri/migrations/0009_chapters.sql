CREATE TABLE IF NOT EXISTS chapters (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    video_id   INTEGER NOT NULL REFERENCES videos(id) ON DELETE CASCADE,
    idx        INTEGER NOT NULL,
    start_secs REAL NOT NULL,
    end_secs   REAL NOT NULL,
    title      TEXT NOT NULL,
    summary    TEXT
);

CREATE INDEX IF NOT EXISTS idx_chapters_video ON chapters(video_id);
