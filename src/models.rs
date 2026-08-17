use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: String,
    pub local_name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Log {
    pub id: String,
    pub user_id: String,
    pub channel: String,
    pub text: String,
    pub content_type: String,
    pub media_path: Option<String>,
    pub mime_type: Option<String>,
    pub file_size: Option<i64>,
    pub telegram_message_id: Option<i64>,
    pub timestamp: String,
    pub privacy_level: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionAnalysis {
    pub overview: String,
    #[serde(default)]
    pub connections: Vec<Connection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub kind: String,
    pub description: String,
    pub confidence: f64,
    pub source_log_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AddResult {
    pub log: Log,
    pub redacted_preview: String,
    pub connections: Vec<Connection>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StorageAction {
    Store,
    Ignore,
    Ask,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageDecision {
    pub action: StorageAction,
}

#[derive(Debug, Serialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum IngestResult {
    Stored { result: Box<AddResult> },
    Ignored,
    Ask,
}
