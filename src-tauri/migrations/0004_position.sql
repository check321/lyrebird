-- 断点续播：记录每个视频的播放位置（秒）
ALTER TABLE videos ADD COLUMN position_secs REAL NOT NULL DEFAULT 0;
