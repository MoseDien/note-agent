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
    pub sentiment: Option<String>,
    pub importance: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub category: String,
    pub summary: String,
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default)]
    pub entities: Vec<EntityMention>,
    pub sentiment: String,
    pub importance: u8,
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
