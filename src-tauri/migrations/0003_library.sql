-- 媒体库扩展：频道、简介、封面、用户可编辑分类
ALTER TABLE videos ADD COLUMN channel TEXT;
ALTER TABLE videos ADD COLUMN channel_id TEXT;
ALTER TABLE videos ADD COLUMN description TEXT;
ALTER TABLE videos ADD COLUMN thumbnail_path TEXT;
-- 分类：导入时默认取频道名，用户可改
ALTER TABLE videos ADD COLUMN category TEXT NOT NULL DEFAULT '未分类';
