use crate::{
    config::Config,
    db::Store,
    glm::GlmClient,
    models::{AddResult, ConnectionAnalysis, Log},
    privacy,
};
use anyhow::Result;

pub async fn add_log(
    store: &Store,
    config: &Config,
    user_id: &str,
    text: &str,
    channel: &str,
    privacy_level: &str,
) -> Result<AddResult> {
    anyhow::ensure!(!text.trim().is_empty(), "日志内容不能为空");
    anyhow::ensure!(
        matches!(privacy_level, "normal" | "no_upload"),
        "无效隐私级别"
    );
    let mut log = store
        .insert_log(user_id, channel, text.trim(), privacy_level)
        .await?;
    let redacted = privacy::redact(text.trim());
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
                    let mut context = vec![log_for_model(&log)];
                    context.extend(candidates.iter().map(log_for_model));
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
            overview: "至少需要两条记录才能分析联系。".into(),
            connections: vec![],
        });
    }
    let input_logs: Vec<serde_json::Value> = logs.iter().map(log_for_model).collect();
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

fn log_for_model(log: &Log) -> serde_json::Value {
    serde_json::json!({
        "id": log.id,
        "created_at": log.created_at,
        "category": log.category,
        "summary": log.summary.as_deref().map(privacy::redact),
        "topics": log.topics_json
    })
}

pub fn format_log(log: &Log) -> String {
    format!(
        "[{}] {} · {}\n{}",
        log.id,
        log.created_at,
        log.category.as_deref().unwrap_or("待分析"),
        log.summary.as_deref().unwrap_or(&log.text)
    )
}
