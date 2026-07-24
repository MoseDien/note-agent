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
    pub privacy_level: String,
    pub analysis_status: String,
    pub category: Option<String>,
    pub summary: Option<String>,
    pub topics_json: Option<String>,
    pub primary_tag: Option<String>,
    pub system_tags_json: String,
    pub topic_tags_json: String,
    pub details_json: String,
    pub tag_schema_version: i64,
    pub sentiment: Option<String>,
    pub importance: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub primary_tag: String,
    #[serde(default)]
    pub system_tags: Vec<String>,
    #[serde(default)]
    pub topic_tags: Vec<String>,
    #[serde(default = "empty_object")]
    pub details: serde_json::Value,
    // Legacy fields retained during the compatibility migration.
    pub category: String,
    pub summary: String,
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default)]
    pub entities: Vec<EntityMention>,
    pub sentiment: String,
    pub importance: u8,
}

fn empty_object() -> serde_json::Value {
    serde_json::json!({})
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityMention {
    pub kind: String,
    pub name: String,
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
    pub confidence: f32,
    pub reason_code: String,
    pub analysis: Analysis,
}

#[derive(Debug, Serialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum IngestResult {
    Stored {
        analysis: Box<AddResult>,
        confidence: f32,
        reason_code: String,
    },
    Ignored {
        confidence: f32,
        reason_code: String,
    },
    Ask {
        confidence: f32,
        reason_code: String,
    },
}
