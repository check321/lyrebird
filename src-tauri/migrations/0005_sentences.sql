-- 阅读区用的 AI 断句分段结果（与逐 cue 的字幕区分开：cue 供视频内嵌字幕，sentence 供阅读）
CREATE TABLE IF NOT EXISTS sentences (
    id         INTEGER PRIMARY KEY,
    video_id   INTEGER NOT NULL REFERENCES videos(id) ON DELETE CASCADE,
    para_idx   INTEGER NOT NULL,
    sent_idx   INTEGER NOT NULL,
    start_secs REAL NOT NULL,
    end_secs   REAL NOT NULL,
    text_en    TEXT NOT NULL,
    text_zh    TEXT
);

CREATE INDEX IF NOT EXISTS idx_sentences_video ON sentences (video_id, para_idx, sent_idx);
