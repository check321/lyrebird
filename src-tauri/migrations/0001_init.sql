-- Lyrebird 初始数据库结构

CREATE TABLE IF NOT EXISTS videos (
    id              INTEGER PRIMARY KEY,
    url             TEXT NOT NULL UNIQUE,
    youtube_id      TEXT,
    title           TEXT,
    duration_secs   REAL,
    video_path      TEXT,
    audio_path      TEXT,
    subtitle_source TEXT CHECK (subtitle_source IN ('youtube', 'asr')),
    tldr            TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 字幕段落；中文翻译直接存于 text_zh，避免额外表 join
CREATE TABLE IF NOT EXISTS cues (
    id          INTEGER PRIMARY KEY,
    video_id    INTEGER NOT NULL REFERENCES videos(id) ON DELETE CASCADE,
    idx         INTEGER NOT NULL,
    start_secs  REAL NOT NULL,
    end_secs    REAL NOT NULL,
    text_en     TEXT NOT NULL,
    text_zh     TEXT,
    speaker     TEXT,
    UNIQUE (video_id, idx)
);

CREATE INDEX IF NOT EXISTS idx_cues_video_time ON cues (video_id, start_secs);

CREATE TABLE IF NOT EXISTS word_cards (
    id          INTEGER PRIMARY KEY,
    word        TEXT NOT NULL,
    phonetic    TEXT,
    definition  TEXT,  -- ECDICT 本地词典释义
    ai_analysis TEXT,  -- LLM 上下文深度解析（按需生成并缓存）
    video_id    INTEGER REFERENCES videos(id) ON DELETE SET NULL,
    cue_id      INTEGER REFERENCES cues(id) ON DELETE SET NULL,
    context     TEXT,  -- 例句上下文片段
    start_secs  REAL,  -- 出处时间点，便于跳回原视频
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (word, video_id, cue_id)
);

CREATE INDEX IF NOT EXISTS idx_word_cards_word ON word_cards (word);
