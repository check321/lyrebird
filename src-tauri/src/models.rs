use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Video {
    pub id: i64,
    pub url: String,
    pub youtube_id: Option<String>,
    pub title: Option<String>,
    pub duration_secs: Option<f64>,
    pub video_path: Option<String>,
    pub audio_path: Option<String>,
    pub subtitle_source: Option<String>,
    pub tldr: Option<String>,
    pub created_at: String,
    pub channel: Option<String>,
    pub channel_id: Option<String>,
    pub description: Option<String>,
    pub thumbnail_path: Option<String>,
    pub category: String,
    pub position_secs: f64,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Cue {
    pub id: i64,
    pub video_id: i64,
    pub idx: i64,
    pub start_secs: f64,
    pub end_secs: f64,
    pub text_en: String,
    pub text_zh: Option<String>,
    pub speaker: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Sentence {
    pub id: i64,
    pub video_id: i64,
    pub para_idx: i64,
    pub sent_idx: i64,
    pub start_secs: f64,
    pub end_secs: f64,
    pub text_en: String,
    pub text_zh: Option<String>,
    pub cue_from: i64,
    pub cue_to: i64,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct WordCard {
    pub id: i64,
    pub word: String,
    pub phonetic: Option<String>,
    pub definition: Option<String>,
    pub ai_analysis: Option<String>,
    pub video_id: Option<i64>,
    pub cue_id: Option<i64>,
    pub context: Option<String>,
    pub start_secs: Option<f64>,
    pub created_at: String,
    #[sqlx(default)]
    pub video_title: Option<String>,
    #[sqlx(default)]
    pub video_thumb: Option<String>,
    #[sqlx(default)]
    pub review_count: i64,
    #[sqlx(default)]
    pub fail_streak: i64,
    #[sqlx(default)]
    pub mastered: i64,
    #[sqlx(default)]
    pub last_reviewed_at: Option<String>,
}
