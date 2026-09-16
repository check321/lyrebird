mod asr;
mod channel;
mod commands;
mod db;
mod dict;
mod export;
mod ingest;
mod llm;
mod mdx;
mod media;
mod models;
mod settings;
mod subtitle;
mod translate;
mod words;

use asr::*;
use channel::*;
use commands::*;
use export::*;
use llm::*;
use mdx::*;
use media::*;
use models::Video;
use settings::*;
use sqlx::SqlitePool;
use tauri::Manager;
use translate::*;
use words::*;

#[tauri::command]
async fn list_videos(pool: tauri::State<'_, SqlitePool>) -> Result<Vec<Video>, String> {
    sqlx::query_as::<_, Video>("SELECT * FROM videos ORDER BY created_at DESC")
        .fetch_all(pool.inner())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn save_position(
    pool: tauri::State<'_, SqlitePool>,
    id: i64,
    secs: f64,
) -> Result<(), String> {
    sqlx::query("UPDATE videos SET position_secs = ? WHERE id = ?")
        .bind(secs)
        .bind(id)
        .execute(pool.inner())
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(ExportState::default())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let media_dir = data_dir.join("media");
            std::fs::create_dir_all(&media_dir)?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                match media::serve(media_dir.clone()).await {
                    Ok(port) => {
                        handle.manage(port);
                    }
                    Err(e) => {
                        eprintln!("媒体服务器启动失败: {e}");
                    }
                }
                let pool = db::init(&data_dir.join("lyrebird.db"))
                    .await
                    .expect("failed to initialize database");
                handle.manage(pool);
                handle.manage(media_dir);
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_videos,
            import_video,
            fetch_channel,
            subscribe_channel,
            unsubscribe_channel,
            list_subscriptions,
            get_video,
            list_cues,
            delete_video,
            update_video,
            rename_category,
            get_settings,
            save_settings,
            test_llm_connection,
            translate_video,
            generate_tldr,
            generate_chapters,
            list_chapters,
            structure_video,
            fill_sentence_zh,
            list_sentences,
            update_sentence,
            lookup_word,
            speak_word,
            import_mdx,
            list_mdx_sources,
            analyze_word,
            add_word_card,
            list_word_cards,
            list_saved_words,
            delete_word_card,
            list_review_candidates,
            random_distractors,
            submit_review,
            save_review_round,
            list_review_rounds,
            review_stats,
            transcribe_video,
            check_asr_env,
            export_video,
            cancel_export,
            ensure_export_fonts,
            ffmpeg_features,
            video_storyboard,
            reveal_in_folder,
            media_url,
            save_position,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
