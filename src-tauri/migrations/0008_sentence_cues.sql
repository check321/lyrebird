-- 句子级记录其覆盖的 cue 区间，使翻译后无需重跑 LLM 即可本地拼接句级中文
ALTER TABLE sentences ADD COLUMN cue_from INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sentences ADD COLUMN cue_to INTEGER NOT NULL DEFAULT 0;
