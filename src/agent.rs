use crate::{
    config::Config,
    db::Store,
    glm::GlmClient,
    local_llm::LocalClassifier,
    models::{AddResult, Analysis, ConnectionAnalysis, IngestResult, Log, StorageAction},
    privacy,
};
use anyhow::Result;

pub async fn ingest_log(
    store: &Store,
    config: &Config,
    local_classifier: &LocalClassifier,
    user_id: &str,
    text: &str,
    channel: &str,
) -> Result<IngestResult> {
    anyhow::ensure!(!text.trim().is_empty(), config.i18n.text("error.empty_log"));
    let decision = match local_classifier.decide(text.trim()).await {
        Ok(decision) => decision,
        Err(error) => {
            tracing::warn!(error=%error, "local classification failed");
            return Ok(IngestResult::Ask {
                confidence: 0.0,
                reason_code: "local_model_unavailable".into(),
            });
        }
    };
    Ok(match decision.action {
        StorageAction::Store => IngestResult::Stored {
            analysis: Box::new(
                save_log(
                    store,
                    config,
                    user_id,
                    text,
                    channel,
                    "normal",
                    Some(&decision.analysis),
                )
                .await?,
            ),
            confidence: decision.confidence,
            reason_code: decision.reason_code,
        },
        StorageAction::Ignore => IngestResult::Ignored {
            confidence: decision.confidence,
            reason_code: decision.reason_code,
        },
        StorageAction::Ask => IngestResult::Ask {
            confidence: decision.confidence,
            reason_code: decision.reason_code,
        },
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
    let local = LocalClassifier::from_config(config).await?;
    let analysis = if privacy_level == "no_upload" {
        None
    } else {
        match local.decide(text.trim()).await {
            Ok(decision) => Some(decision.analysis),
            Err(error) => {
                tracing::warn!(error=%error, "forced log saved without local analysis");
                None
            }
        }
    };
    save_log(
        store,
        config,
        user_id,
        text,
        channel,
        privacy_level,
        analysis.as_ref(),
    )
    .await
}

async fn save_log(
    store: &Store,
    config: &Config,
    user_id: &str,
    text: &str,
    channel: &str,
    privacy_level: &str,
    analysis: Option<&Analysis>,
) -> Result<AddResult> {
    anyhow::ensure!(!text.trim().is_empty(), config.i18n.text("error.empty_log"));
    anyhow::ensure!(
        matches!(privacy_level, "normal" | "no_upload"),
        config.i18n.text("error.invalid_privacy")
    );
    let mut log = store
        .insert_log(user_id, channel, text.trim(), privacy_level)
        .await?;
    let redacted = privacy::redact(text.trim(), &config.i18n);
    if let Some(analysis) = analysis {
        store.save_analysis(&log.id, analysis).await?;
        log = store.get_log(&log.id).await?;
    }
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
    let logs = store.recent_logs(user_id, limit).await?;
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
        "category": log.category,
        "summary": log.summary.as_deref().map(|value| privacy::redact(value, &config.i18n)),
        "topics": log.topics_json
    })
}

pub fn format_log(log: &Log, config: &Config) -> String {
    format!(
        "[{}] {} · {}\n{}",
        log.id,
        log.created_at,
        config.i18n.category(log.category.as_deref()),
        log.summary.as_deref().unwrap_or(&log.text)
    )
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
        local_llm::LocalClassifier,
        models::{Analysis, IngestResult, StorageAction, StorageDecision},
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
        assert_eq!(private.log.analysis_status, "not_requested");
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
        assert_eq!(pending.log.analysis_status, "pending");
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
        assert!(format_log(&pending.log, &config).contains("Pending analysis"));

        let ignore = LocalClassifier::from_test_decision(StorageDecision {
            action: StorageAction::Ignore,
            confidence: 0.9,
            reason_code: "question".into(),
            analysis: Analysis {
                category: "other".into(),
                summary: "question".into(),
                topics: vec![],
                entities: vec![],
                sentiment: "neutral".into(),
                importance: 1,
            },
        });
        assert!(matches!(
            ingest_log(&store, &config, &ignore, &user.id, "a question", "terminal")
                .await
                .unwrap(),
            IngestResult::Ignored { .. }
        ));
        let ask = LocalClassifier::from_test_decision(StorageDecision {
            action: StorageAction::Ask,
            confidence: 0.5,
            reason_code: "ambiguous".into(),
            analysis: Analysis {
                category: "other".into(),
                summary: "ambiguous".into(),
                topics: vec![],
                entities: vec![],
                sentiment: "neutral".into(),
                importance: 1,
            },
        });
        assert!(matches!(
            ingest_log(&store, &config, &ask, &user.id, "ambiguous", "terminal")
                .await
                .unwrap(),
            IngestResult::Ask { .. }
        ));
        assert_eq!(store.recent_logs(&user.id, 10).await.unwrap().len(), 2);

        let store_gate = LocalClassifier::disabled();
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
}
