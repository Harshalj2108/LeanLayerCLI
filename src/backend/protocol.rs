use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BackendMessage {
    Ready,
    Token { content: String },
    Done,
    Error { message: String },
    Pong,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FrontendMessage {
    Generate { messages: Vec<ChatMessage> },
    Ping,
    Quit,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,   // "user" | "assistant" | "system"
    pub content: String,
}