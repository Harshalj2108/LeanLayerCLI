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

/// Chat message with optional multimodal image attachments.
/// Internally `content` stays as String for easy manipulation.
/// Images are base64-encoded data URIs ("data:image/png;base64,...").
/// The backend serializer converts to the provider-specific multimodal format.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,   // "user" | "assistant" | "system"
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>,
}

impl ChatMessage {
    /// Create a simple text message
    pub fn text(role: &str, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            images: None,
        }
    }

    /// Create a multimodal message with text and images
    pub fn multimodal(role: &str, content: impl Into<String>, images: Vec<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            images: if images.is_empty() { None } else { Some(images) },
        }
    }

    /// Check if this message has image attachments
    pub fn has_images(&self) -> bool {
        self.images.as_ref().map_or(false, |imgs| !imgs.is_empty())
    }
}