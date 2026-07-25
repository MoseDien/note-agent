use crate::{
    config::Config,
    db::Store,
    glm::GlmClient,
    local_llm::LocalStorageGate,
    models::{AddResult, ConnectionAnalysis, IngestResult, Log, StorageAction},
    privacy,
};
use anyhow::Result;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct ReversalStore {
    entries: Arc<Mutex<HashMap<(String, String), ReversibleDecision>>>,
    ttl: Duration,
}

#[derive(Clone)]
enum ReversibleDecision {
    Stored {
        log_id: String,
        recorded_at: std::time::Instant,
    },
    NotStored {
        text: String,
        recorded_at: std::time::Instant,
    },
}

pub enum ReversalOutcome {
    Stored(AddResult),
    Deleted { log_id: String },
    Unavailable,
}

impl Default for ReversalStore {
    fn default() -> Self {
        Self::new(Duration::from_secs(10 * 60))
    }
}

impl ReversalStore {
    pub fn new(ttl: Duration) -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
            ttl,
        }
    }

    pub async fn remember(&self, user_id: &str, channel: &str, text: &str, result: &IngestResult) {
        let decision = match result {
            IngestResult::Stored { result } => ReversibleDecision::Stored {
                log_id: result.log.id.clone(),
                recorded_at: std::time::Instant::now(),
            },
            IngestResult::Ignored | IngestResult::Ask => ReversibleDecision::NotStored {
                text: text.to_owned(),
                recorded_at: std::time::Instant::now(),
            },
        };
        self.entries
            .lock()
            .await
            .insert((user_id.to_owned(), channel.to_owned()), decision);
    }

    async fn take(&self, user_id: &str, channel: &str) -> Option<ReversibleDecision> {
        let decision = self
            .entries
            .lock()
            .await
            .remove(&(user_id.to_owned(), channel.to_owned()))?;
        let recorded_at = match &decision {
            ReversibleDecision::Stored { recorded_at, .. }
            | ReversibleDecision::NotStored { recorded_at, .. } => *recorded_at,
        };
        (recorded_at.elapsed() <= self.ttl).then_some(decision)
    }
}

pub async fn reverse_last_decision(
    store: &Store,
    config: &Config,
    reversals: &ReversalStore,
    user_id: &str,
    channel: &str,
) -> Result<ReversalOutcome> {
    Ok(match reversals.take(user_id, channel).await {
        Some(ReversibleDecision::Stored { log_id, .. }) => {
            if store.delete_log(user_id, &log_id).await? {
                ReversalOutcome::Deleted { log_id }
            } else {
                ReversalOutcome::Unavailable
            }
        }
        Some(ReversibleDecision::NotStored { text, .. }) => ReversalOutcome::Stored(
            add_log(store, config, user_id, &text, channel, "normal").await?,
        ),
        None => ReversalOutcome::Unavailable,
    })
}

pub async fn ingest_log(
    store: &Store,
    config: &Config,
    storage_gate: &LocalStorageGate,
    user_id: &str,
    text: &str,
    channel: &str,
) -> Result<IngestResult> {
    anyhow::ensure!(!text.trim().is_empty(), config.i18n.text("error.empty_log"));
    let decision = match storage_gate.decide(text.trim()).await {
        Ok(decision) => decision,
        Err(error) => {
            tracing::warn!(error=%error, "local classification failed");
            return Ok(IngestResult::Ask);
        }
    };
    Ok(match decision.action {
        StorageAction::Store => IngestResult::Stored {
            result: Box::new(save_log(store, config, user_id, text, channel, "normal").await?),
        },
        StorageAction::Ignore => IngestResult::Ignored,
        StorageAction::Ask => IngestResult::Ask,
    })
}

pub async fn add_log(
    store: &Store,
    config: &Config,
    user_id: &str,
    text: &str,
    channel: &str,
    privacy_level: &str,
) -> Result<AddResult> {
    save_log(store, config, user_id, text, channel, privacy_level).await
}

async fn save_log(
    store: &Store,
    config: &Config,
    user_id: &str,
    text: &str,
    channel: &str,
    privacy_level: &str,
) -> Result<AddResult> {
    anyhow::ensure!(!text.trim().is_empty(), config.i18n.text("error.empty_log"));
    anyhow::ensure!(
        matches!(privacy_level, "normal" | "no_upload"),
        config.i18n.text("error.invalid_privacy")
    );
    let log = store
        .insert_log(user_id, channel, text.trim(), privacy_level)
        .await?;
    let redacted = privacy::redact(text.trim(), &config.i18n);
    Ok(AddResult {
        log,
        redacted_preview: redacted,
        connections: vec![],
    })
}

pub async fn connections(
    store: &Store,
    config: &Config,
    user_id: &str,
    limit: u32,
) -> Result<ConnectionAnalysis> {
    let logs: Vec<Log> = store
        .recent_logs(user_id, limit)
        .await?
        .into_iter()
        .filter(|log| log.privacy_level != "no_upload")
        .collect();
    if logs.len() < 2 {
        return Ok(ConnectionAnalysis {
            overview: config.i18n.text("analysis.need_two_logs"),
            connections: vec![],
        });
    }
    let input_logs: Vec<serde_json::Value> =
        logs.iter().map(|log| log_for_model(log, config)).collect();
    let result = GlmClient::from_config(config)?
        .connections(&serde_json::to_string(&input_logs)?)
        .await?;
    let valid_ids: std::collections::HashSet<&str> = logs.iter().map(|l| l.id.as_str()).collect();
    let mut validated = result;
    validated.connections.retain(|c| {
        c.source_log_ids.len() >= 2
            && c.source_log_ids
                .iter()
                .all(|id| valid_ids.contains(id.as_str()))
            && (0.0..=1.0).contains(&c.confidence)
    });
    store
        .save_connections(user_id, &validated.connections)
        .await?;
    Ok(validated)
}

fn log_for_model(log: &Log, config: &Config) -> serde_json::Value {
    serde_json::json!({
        "id": log.id,
        "created_at": log.created_at,
        "text": privacy::redact(&log.text, &config.i18n)
    })
}

pub fn format_log(log: &Log, _config: &Config) -> String {
    format!("[{}] {}\n{}", log.id, log.created_at, log.text)
}

pub async fn delete_log_reference(
    store: &Store,
    config: &Config,
    user_id: &str,
    reference: &str,
) -> Result<bool> {
    if let Some(position) = reference.strip_prefix('-') {
        let position: u32 = position
            .parse()
            .map_err(|_| anyhow::anyhow!(config.i18n.text("error.invalid_delete_position")))?;
        anyhow::ensure!(
            (1..=10).contains(&position),
            config.i18n.text("error.invalid_delete_position")
        );
        let logs = store.recent_logs(user_id, position).await?;
        return match logs.get((position - 1) as usize) {
            Some(log) => store.delete_log(user_id, &log.id).await,
            None => Ok(false),
        };
    }
    store.delete_log(user_id, reference).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        local_llm::LocalStorageGate,
        models::{IngestResult, StorageAction, StorageDecision},
        test_support::{config as test_config, glm_response, mock_server},
    };
    use uuid::Uuid;

    async fn store() -> (Store, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("daily-agent-agent-{}.db", Uuid::new_v4()));
        let store = Store::connect(&format!("sqlite://{}", path.display()))
            .await
            .unwrap();
        store.migrate().await.unwrap();
        (store, path)
    }

    #[tokio::test]
    async fn no_upload_and_missing_key_preserve_logs_without_network() {
        let (store, path) = store().await;
        let user = store.ensure_local_user("owner").await.unwrap();
        let config = test_config(
            format!("sqlite://{}", path.display()),
            "http://unused".into(),
            None,
        );
        let private = add_log(
            &store,
            &config,
            &user.id,
            "phone 13812345678",
            "terminal",
            "no_upload",
        )
        .await
        .unwrap();
        assert!(private.redacted_preview.contains("[PHONE]"));

        let pending = add_log(
            &store,
            &config,
            &user.id,
            "ordinary log",
            "terminal",
            "normal",
        )
        .await
        .unwrap();
        assert!(
            add_log(&store, &config, &user.id, "", "terminal", "normal")
                .await
                .is_err()
        );
        assert!(
            add_log(&store, &config, &user.id, "text", "terminal", "bad")
                .await
                .is_err()
        );
        assert!(format_log(&pending.log, &config).contains("ordinary log"));

        let ignore = LocalStorageGate::from_test_decision(StorageDecision {
            action: StorageAction::Ignore,
        });
        assert!(matches!(
            ingest_log(&store, &config, &ignore, &user.id, "a question", "terminal")
                .await
                .unwrap(),
            IngestResult::Ignored
        ));
        let ask = LocalStorageGate::from_test_decision(StorageDecision {
            action: StorageAction::Ask,
        });
        assert!(matches!(
            ingest_log(&store, &config, &ask, &user.id, "ambiguous", "terminal")
                .await
                .unwrap(),
            IngestResult::Ask
        ));
        assert_eq!(store.recent_logs(&user.id, 10).await.unwrap().len(), 2);

        let store_gate = LocalStorageGate::disabled();
        assert!(matches!(
            ingest_log(
                &store,
                &config,
                &store_gate,
                &user.id,
                "store this",
                "terminal"
            )
            .await
            .unwrap(),
            IngestResult::Stored { .. }
        ));
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn explicit_connections_validate_model_boundaries() {
        let (store, path) = store().await;
        let user = store.ensure_local_user("owner").await.unwrap();
        store
            .insert_log(&user.id, "terminal", "first", "normal")
            .await
            .unwrap();
        store
            .insert_log(&user.id, "terminal", "second", "normal")
            .await
            .unwrap();
        let logs = store.recent_logs(&user.id, 30).await.unwrap();
        let valid = serde_json::json!({
            "overview":"overview",
            "connections":[
                {"kind":"shared_topic","description":"valid","confidence":0.8,"source_log_ids":[logs[0].id,logs[1].id]},
                {"kind":"shared_topic","description":"bad confidence","confidence":2.0,"source_log_ids":[logs[0].id,logs[1].id]},
                {"kind":"shared_topic","description":"bad id","confidence":0.8,"source_log_ids":[logs[0].id,"missing"]}
            ]
        });
        let (url, handle) = mock_server(vec![glm_response(&valid.to_string())]).await;
        let config = test_config(format!("sqlite://{}", path.display()), url, Some("key"));
        let result = connections(&store, &config, &user.id, 30).await.unwrap();
        assert_eq!(result.connections.len(), 1);
        handle.await.unwrap();

        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn connections_never_upload_no_upload_logs() {
        let (store, path) = store().await;
        let user = store.ensure_local_user("private-owner").await.unwrap();
        store
            .insert_log(&user.id, "terminal", "normal", "normal")
            .await
            .unwrap();
        store
            .insert_log(&user.id, "terminal", "private", "no_upload")
            .await
            .unwrap();
        let config = test_config(
            format!("sqlite://{}", path.display()),
            "http://must-not-be-called".into(),
            Some("key"),
        );

        let result = connections(&store, &config, &user.id, 30).await.unwrap();
        assert!(result.connections.is_empty());
        assert_eq!(result.overview, config.i18n.text("analysis.need_two_logs"));

        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn deletes_recent_log_by_bounded_position() {
        let (store, path) = store().await;
        let user = store.ensure_local_user("owner").await.unwrap();
        for text in ["first", "second", "third"] {
            store
                .insert_log(&user.id, "terminal", text, "normal")
                .await
                .unwrap();
        }
        let config = test_config(
            format!("sqlite://{}", path.display()),
            "http://unused".into(),
            None,
        );
        assert!(
            delete_log_reference(&store, &config, &user.id, "-2")
                .await
                .unwrap()
        );
        let remaining = store.recent_logs(&user.id, 10).await.unwrap();
        assert_eq!(remaining.len(), 2);
        assert!(!remaining.iter().any(|log| log.text == "second"));
        assert!(
            delete_log_reference(&store, &config, &user.id, "-11")
                .await
                .is_err()
        );
        assert!(
            delete_log_reference(&store, &config, &user.id, "-0")
                .await
                .is_err()
        );
        assert!(
            !delete_log_reference(&store, &config, &user.id, "-3")
                .await
                .unwrap()
        );
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn reverses_storage_decisions_once_with_user_and_channel_isolation() {
        let (store, path) = store().await;
        let user = store.ensure_local_user("owner").await.unwrap();
        let other = store.ensure_local_user("other").await.unwrap();
        let config = test_config(
            format!("sqlite://{}", path.display()),
            "http://unused".into(),
            None,
        );
        let reversals = ReversalStore::default();

        reversals
            .remember(&user.id, "terminal", "keep this", &IngestResult::Ignored)
            .await;
        assert!(matches!(
            reverse_last_decision(&store, &config, &reversals, &other.id, "terminal")
                .await
                .unwrap(),
            ReversalOutcome::Unavailable
        ));
        assert!(matches!(
            reverse_last_decision(&store, &config, &reversals, &user.id, "telegram")
                .await
                .unwrap(),
            ReversalOutcome::Unavailable
        ));
        assert!(matches!(
            reverse_last_decision(&store, &config, &reversals, &user.id, "terminal")
                .await
                .unwrap(),
            ReversalOutcome::Stored(_)
        ));
        assert_eq!(store.recent_logs(&user.id, 10).await.unwrap().len(), 1);
        assert!(matches!(
            reverse_last_decision(&store, &config, &reversals, &user.id, "terminal")
                .await
                .unwrap(),
            ReversalOutcome::Unavailable
        ));

        let result = ingest_log(
            &store,
            &config,
            &LocalStorageGate::disabled(),
            &user.id,
            "remove this",
            "terminal",
        )
        .await
        .unwrap();
        reversals
            .remember(&user.id, "terminal", "remove this", &result)
            .await;
        assert!(matches!(
            reverse_last_decision(&store, &config, &reversals, &user.id, "terminal")
                .await
                .unwrap(),
            ReversalOutcome::Deleted { .. }
        ));
        assert_eq!(store.recent_logs(&user.id, 10).await.unwrap().len(), 1);

        let expired = ReversalStore::new(Duration::ZERO);
        expired
            .remember(&user.id, "terminal", "expired", &IngestResult::Ask)
            .await;
        assert!(matches!(
            reverse_last_decision(&store, &config, &expired, &user.id, "terminal")
                .await
                .unwrap(),
            ReversalOutcome::Unavailable
        ));

        drop(store);
        let _ = std::fs::remove_file(path);
    }
}
