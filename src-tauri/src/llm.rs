//! OpenAI 兼容 LLM 客户端（DeepSeek 等）。

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;

const DEFAULT_BASE_URL: &str = "https://api.deepseek.com";
const DEFAULT_MODEL: &str = "deepseek-chat";

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("请先在设置页配置 {0}")]
    NotConfigured(&'static str),
    #[error("请求失败: {0}")]
    Http(String),
    #[error("接口返回错误 ({status}): {body}")]
    Api { status: u16, body: String },
    #[error("响应解析失败: {0}")]
    Parse(String),
}

impl LlmConfig {
    /// 从 settings 表读取，缺失项回落默认值。
    pub async fn from_settings(pool: &SqlitePool) -> Result<Self, LlmError> {
        let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM settings")
            .fetch_all(pool)
            .await
            .map_err(|e| LlmError::Http(e.to_string()))?;
        let map: HashMap<String, String> = rows.into_iter().collect();
        let get = |k: &str| map.get(k).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        Ok(LlmConfig {
            base_url: get("llm_base_url").unwrap_or_else(|| DEFAULT_BASE_URL.into()),
            api_key: get("llm_api_key").ok_or(LlmError::NotConfigured("LLM API Key"))?,
            model: get("llm_model").unwrap_or_else(|| DEFAULT_MODEL.into()),
        })
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<Message<'a>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

#[derive(Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: MsgOut,
}

#[derive(Deserialize)]
struct MsgOut {
    content: String,
}

/// 非流式对话调用。`json_mode` 要求模型输出 JSON 对象（接口不支持时自动降级）。
pub async fn chat(
    cfg: &LlmConfig,
    system: &str,
    user: &str,
    json_mode: bool,
) -> Result<String, LlmError> {
    match chat_once(cfg, system, user, json_mode).await {
        Err(LlmError::Api { status: 400, .. }) if json_mode => {
            // 部分推理模型不支持 response_format，降级为普通输出
            chat_once(cfg, system, user, false).await
        }
        r => r,
    }
}

async fn chat_once(
    cfg: &LlmConfig,
    system: &str,
    user: &str,
    json_mode: bool,
) -> Result<String, LlmError> {
    let req = ChatRequest {
        model: &cfg.model,
        messages: vec![
            Message {
                role: "system",
                content: system,
            },
            Message {
                role: "user",
                content: user,
            },
        ],
        stream: false,
        max_tokens: None,
        response_format: json_mode.then_some(ResponseFormat { kind: "json_object" }),
    };
    let resp = reqwest::Client::new()
        .post(cfg.chat_url())
        .bearer_auth(&cfg.api_key)
        .json(&req)
        .send()
        .await
        .map_err(|e| LlmError::Http(e.to_string()))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        let body = resp.text().await.unwrap_or_default();
        let body: String = body.chars().take(300).collect();
        return Err(LlmError::Api { status, body });
    }
    let parsed: ChatResponse = resp
        .json()
        .await
        .map_err(|e| LlmError::Parse(e.to_string()))?;
    parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| LlmError::Parse("choices 为空或 content 为空".into()))
}

/// 用最小开销验证 base_url / api_key / model 是否可用。
#[tauri::command]
pub async fn test_llm_connection(
    base_url: String,
    api_key: String,
    model: String,
) -> Result<String, String> {
    let cfg = LlmConfig {
        base_url: if base_url.trim().is_empty() {
            DEFAULT_BASE_URL.into()
        } else {
            base_url.trim().into()
        },
        api_key: api_key.trim().into(),
        model: if model.trim().is_empty() {
            DEFAULT_MODEL.into()
        } else {
            model.trim().into()
        },
    };
    if cfg.api_key.is_empty() {
        return Err(LlmError::NotConfigured("API Key").to_string());
    }
    chat(&cfg, "You are a connectivity probe.", "ping", false)
        .await
        .map(|_| format!("连接成功（模型 {} 可用）", cfg.model))
        .map_err(|e| e.to_string())
}
