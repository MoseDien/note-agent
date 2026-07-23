use crate::{
    config::Config,
    db::Store,
    glm::GlmClient,
    models::{AddResult, ConnectionAnalysis, IngestResult, Log, StorageAction},
    privacy,
    storage::StorageGate,
};
use anyhow::Result;

pub async fn ingest_log(
    store: &Store,
    config: &Config,
    storage_gate: &StorageGate,
    user_id: &str,
    text: &str,
    channel: &str,
) -> Result<IngestResult> {
    anyhow::ensure!(!text.trim().is_empty(), config.i18n.text("error.empty_log"));
    let decision = storage_gate.decide(text.trim()).await?;
    Ok(match decision.action {
        StorageAction::Store => IngestResult::Stored {
            analysis: Box::new(add_log(store, config, user_id, text, channel, "normal").await?),
            store_score: decision.store_score,
            ignore_score: decision.ignore_score,
        },
        StorageAction::Ignore => IngestResult::Ignored {
            store_score: decision.store_score,
            ignore_score: decision.ignore_score,
        },
        StorageAction::Ask => IngestResult::Ask {
            store_score: decision.store_score,
            ignore_score: decision.ignore_score,
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
    anyhow::ensure!(!text.trim().is_empty(), config.i18n.text("error.empty_log"));
    anyhow::ensure!(
        matches!(privacy_level, "normal" | "no_upload"),
        config.i18n.text("error.invalid_privacy")
    );
    let mut log = store
        .insert_log(user_id, channel, text.trim(), privacy_level)
        .await?;
    let redacted = privacy::redact(text.trim(), &config.i18n);
    let mut found_connections = vec![];
    if privacy_level == "no_upload" {
        return Ok(AddResult {
            log,
            redacted_preview: redacted,
            connections: found_connections,
        });
    }
    match GlmClient::from_config(config) {
        Ok(client) => match client.analyze(&redacted).await {
            Ok(analysis) => {
                store.save_analysis(&log.id, &analysis).await?;
                log = store.get_log(&log.id).await?;
                let query = if analysis.topics.is_empty() {
                    redacted.clone()
                } else {
                    analysis.topics.join(" ")
                };
                let candidates = store
                    .search_candidates(user_id, &query, Some(&log.id), 5)
                    .await?;
                if !candidates.is_empty() {
                    let mut context = vec![log_for_model(&log, config)];
                    context.extend(candidates.iter().map(|log| log_for_model(log, config)));
                    match client.connections(&serde_json::to_string(&context)?).await {
                        Ok(result) => {
                            let valid_ids: std::collections::HashSet<&str> = context
                                .iter()
                                .filter_map(|item| item["id"].as_str())
                                .collect();
                            found_connections = result
                                .connections
                                .into_iter()
                                .filter(|connection| {
                                    connection.source_log_ids.len() >= 2
                                        && connection
                                            .source_log_ids
                                            .iter()
                                            .all(|id| valid_ids.contains(id.as_str()))
                                        && (0.0..=1.0).contains(&connection.confidence)
                                })
                                .collect();
                            store.save_connections(user_id, &found_connections).await?;
                        }
                        Err(error) => {
                            tracing::warn!(log_id=%log.id, error=%error, "GLM connection analysis failed")
                        }
                    }
                }
            }
            Err(error) => {
                tracing::warn!(log_id=%log.id, error=%error, "GLM analysis failed; original log retained");
                store.mark_analysis_failed(&log.id).await?;
                log.analysis_status = "failed".into();
            }
        },
        Err(_) => {
            tracing::info!(log_id=%log.id, "GLM is not configured; original log retained as pending")
        }
    }
    Ok(AddResult {
        log,
        redacted_preview: redacted,
        connections: found_connections,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        models::{Analysis, EntityMention, IngestResult, StorageAction, StorageDecision},
        storage::StorageGate,
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

    fn analysis_json() -> String {
        serde_json::json!({
            "category":"work",
            "summary":"Testing the agent",
            "topics":["Rust","testing"],
            "entities":[{"kind":"project","name":"Daily Agent"}],
            "sentiment":"positive",
            "importance":4
        })
        .to_string()
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

        let ignore = StorageGate::from_test_decision(StorageDecision {
            action: StorageAction::Ignore,
            store_score: 0.2,
            ignore_score: 0.9,
        });
        assert!(matches!(
            ingest_log(&store, &config, &ignore, &user.id, "a question", "terminal")
                .await
                .unwrap(),
            IngestResult::Ignored { .. }
        ));
        let ask = StorageGate::from_test_decision(StorageDecision {
            action: StorageAction::Ask,
            store_score: 0.7,
            ignore_score: 0.68,
        });
        assert!(matches!(
            ingest_log(&store, &config, &ask, &user.id, "ambiguous", "terminal")
                .await
                .unwrap(),
            IngestResult::Ask { .. }
        ));
        assert_eq!(store.recent_logs(&user.id, 10).await.unwrap().len(), 2);

        let store_gate = StorageGate::disabled();
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
    async fn analyzes_new_log_and_discovers_valid_connections() {
        let (store, path) = store().await;
        let user = store.ensure_local_user("owner").await.unwrap();
        let old = store
            .insert_log(&user.id, "terminal", "Rust testing", "normal")
            .await
            .unwrap();
        store
            .save_analysis(
                &old.id,
                &Analysis {
                    category: "work".into(),
                    summary: "Earlier Rust testing".into(),
                    topics: vec!["Rust".into(), "testing".into()],
                    entities: vec![EntityMention {
                        kind: "project".into(),
                        name: "Daily Agent".into(),
                    }],
                    sentiment: "positive".into(),
                    importance: 3,
                },
            )
            .await
            .unwrap();
        let connection = serde_json::json!({
            "overview":"Related work",
            "connections":[{
                "kind":"shared_topic",
                "description":"Both logs discuss Rust testing",
                "confidence":0.9,
                "source_log_ids":[old.id, "invalid-current-placeholder"]
            }]
        });
        let (url, handle) = mock_server(vec![
            glm_response(&analysis_json()),
            glm_response(&connection.to_string()),
        ])
        .await;
        let config = test_config(format!("sqlite://{}", path.display()), url, Some("key"));
        let result = add_log(
            &store,
            &config,
            &user.id,
            "More Rust testing",
            "terminal",
            "normal",
        )
        .await
        .unwrap();
        assert_eq!(result.log.analysis_status, "complete");
        // The model referenced an ID outside the supplied set, so it is rejected.
        assert!(result.connections.is_empty());
        assert_eq!(handle.await.unwrap().len(), 2);
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn marks_failed_analysis_and_handles_connection_boundaries() {
        let (store, path) = store().await;
        let user = store.ensure_local_user("owner").await.unwrap();
        let (url, handle) = mock_server(vec![glm_response("not json")]).await;
        let config = test_config(format!("sqlite://{}", path.display()), url, Some("key"));
        let result = add_log(&store, &config, &user.id, "a log", "terminal", "normal")
            .await
            .unwrap();
        assert_eq!(result.log.analysis_status, "failed");
        handle.await.unwrap();
        let result = connections(&store, &config, &user.id, 30).await.unwrap();
        assert!(result.connections.is_empty());

        store
            .insert_log(&user.id, "terminal", "second", "no_upload")
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
}
