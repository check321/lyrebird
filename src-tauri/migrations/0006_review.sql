-- 生词复习：记忆状态 + 复习轮次记录
ALTER TABLE word_cards ADD COLUMN review_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE word_cards ADD COLUMN fail_streak INTEGER NOT NULL DEFAULT 0;
ALTER TABLE word_cards ADD COLUMN mastered INTEGER NOT NULL DEFAULT 0;
ALTER TABLE word_cards ADD COLUMN last_reviewed_at TEXT;

CREATE TABLE IF NOT EXISTS review_rounds (
    id          INTEGER PRIMARY KEY,
    total       INTEGER NOT NULL,
    correct     INTEGER NOT NULL,
    score       INTEGER NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
