use crate::{
    config::Config,
    models::{Analysis, StorageAction, StorageDecision},
};
use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;

#[derive(Clone)]
pub struct LocalClassifier {
    http: Client,
    url: String,
    model: String,
    prompt: String,
    #[cfg(test)]
    fixed: Option<StorageDecision>,
}

#[derive(Deserialize)]
struct OllamaResponse {
    message: OllamaMessage,
}

#[derive(Deserialize)]
struct OllamaMessage {
    content: String,
}

#[derive(Deserialize)]
struct LocalResult {
    storage_action: StorageAction,
    confidence: f32,
    reason_code: String,
    primary_tag: String,
    system_tags: Vec<String>,
    topic_tags: Vec<String>,
    details: serde_json::Value,
    summary: String,
    #[serde(default)]
    entities: Vec<crate::models::EntityMention>,
    sentiment: String,
    importance: u8,
}

impl LocalClassifier {
    pub async fn from_config(config: &Config) -> Result<Self> {
        Ok(Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(
                    config.local_llm_timeout_seconds,
                ))
                .build()?,
            url: config.local_llm_url.trim_end_matches('/').into(),
            model: config.local_llm_model.clone(),
            prompt: config.prompts.local_classify().into(),
            #[cfg(test)]
            fixed: None,
        })
    }

    #[cfg(test)]
    pub fn from_test_url(url: String, config: &Config) -> Self {
        Self {
            http: Client::new(),
            url,
            model: "test-local-model".into(),
            prompt: config.prompts.local_classify().into(),
            fixed: None,
        }
    }

    #[cfg(test)]
    pub fn from_test_decision(decision: StorageDecision) -> Self {
        let config =
            crate::test_support::config("sqlite::memory:".into(), "http://unused".into(), None);
        let mut gate = Self::from_test_url("http://unused".into(), &config);
        gate.fixed = Some(decision);
        gate
    }

    #[cfg(test)]
    pub fn disabled() -> Self {
        Self::from_test_decision(StorageDecision {
            action: StorageAction::Store,
            confidence: 1.0,
            reason_code: "test".into(),
            analysis: Analysis {
                primary_tag: "activity".into(),
                system_tags: vec!["activity".into()],
                topic_tags: vec![],
                details: serde_json::json!({}),
                category: "other".into(),
                summary: "test".into(),
                topics: vec![],
                entities: vec![],
                sentiment: "neutral".into(),
                importance: 1,
            },
        })
    }

    pub async fn decide(&self, text: &str) -> Result<StorageDecision> {
        let input = serde_json::json!({"input": text}).to_string();
        #[cfg(test)]
        if let Some(decision) = &self.fixed {
            return Ok(decision.clone());
        }
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "storage_action": {"type":"string","enum":["store","ignore","ask"]},
                "confidence": {"type":"number","minimum":0,"maximum":1},
                "reason_code": {"type":"string","maxLength":40},
                "primary_tag": {"type":"string","enum":["reflection","idea","decision","plan",
                    "activity","experience","fact","reminder","lesson","preference",
                    "commitment","question"]},
                "system_tags": {"type":"array","minItems":1,"maxItems":12,"uniqueItems":true,
                    "items":{"type":"string","enum":["reflection","idea","decision","plan",
                    "activity","experience","fact","reminder","lesson","preference",
                    "commitment","question","self","family","relationship","work","project",
                    "learning","health","wellbeing","finance","habit","mood","sleep","stress",
                    "energy","symptom","medication","birthday","deadline","appointment"]}},
                "topic_tags": {"type":"array","maxItems":8,"uniqueItems":true,
                    "items":{"type":"string","maxLength":60}},
                "details": {"type":"object"},
                "summary": {"type":"string","maxLength":200},
                "entities": {"type":"array","items":{"type":"object","properties":{
                    "kind":{"type":"string","maxLength":40},"name":{"type":"string","maxLength":80}
                },"required":["kind","name"]},"maxItems":8},
                "sentiment": {"type":"string","enum":["positive","neutral","negative","mixed"]},
                "importance": {"type":"integer","minimum":1,"maximum":5}
            },
            "required":["storage_action","confidence","reason_code","primary_tag","system_tags",
                "topic_tags","details","summary","entities","sentiment","importance"]
        });
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role":"system","content":self.prompt},
                {"role":"user","content":input}
            ],
            "stream": false,
            "think": false,
            "keep_alive": "30m",
            "format": schema,
            "options": {"temperature":0,"num_predict":512,"num_ctx":2048}
        });
        let response = self
            .http
            .post(format!("{}/api/chat", self.url))
            .json(&body)
            .send()
            .await
            .context("local Ollama request failed")?
            .error_for_status()
            .context("local Ollama returned an error")?
            .json::<OllamaResponse>()
            .await
            .context("invalid Ollama response")?;
        let mut result: LocalResult = serde_json::from_str(response.message.content.trim())
            .context("local model did not return valid JSON")?;
        validate(&result)?;
        normalize_tags(&mut result);
        let category = legacy_category(&result.primary_tag).into();
        Ok(StorageDecision {
            action: result.storage_action,
            confidence: result.confidence,
            reason_code: result.reason_code,
            analysis: Analysis {
                primary_tag: result.primary_tag,
                system_tags: result.system_tags,
                topic_tags: result.topic_tags.clone(),
                details: result.details,
                category,
                summary: result.summary,
                topics: result.topic_tags,
                entities: result.entities,
                sentiment: result.sentiment,
                importance: result.importance,
            },
        })
    }
}

fn validate(result: &LocalResult) -> Result<()> {
    anyhow::ensure!(
        (0.0..=1.0).contains(&result.confidence),
        "invalid local confidence: {}",
        result.confidence
    );
    const PRIMARY_TAGS: &[&str] = &[
        "reflection",
        "idea",
        "decision",
        "plan",
        "activity",
        "experience",
        "fact",
        "reminder",
        "lesson",
        "preference",
        "commitment",
        "question",
    ];
    const SYSTEM_TAGS: &[&str] = &[
        "reflection",
        "idea",
        "decision",
        "plan",
        "activity",
        "experience",
        "fact",
        "reminder",
        "lesson",
        "preference",
        "commitment",
        "question",
        "self",
        "family",
        "relationship",
        "work",
        "project",
        "learning",
        "health",
        "wellbeing",
        "finance",
        "habit",
        "mood",
        "sleep",
        "stress",
        "energy",
        "symptom",
        "medication",
        "birthday",
        "deadline",
        "appointment",
    ];
    anyhow::ensure!(
        PRIMARY_TAGS.contains(&result.primary_tag.as_str()),
        "invalid primary tag"
    );
    anyhow::ensure!(
        !result.system_tags.is_empty()
            && result
                .system_tags
                .iter()
                .all(|tag| SYSTEM_TAGS.contains(&tag.as_str())),
        "invalid system tags"
    );
    anyhow::ensure!(result.details.is_object(), "details must be an object");
    anyhow::ensure!(
        ["positive", "neutral", "negative", "mixed"].contains(&result.sentiment.as_str()),
        "invalid local sentiment"
    );
    anyhow::ensure!(
        (1..=5).contains(&result.importance),
        "invalid local importance"
    );
    Ok(())
}

fn normalize_tags(result: &mut LocalResult) {
    if !result.system_tags.contains(&result.primary_tag) {
        result.system_tags.insert(0, result.primary_tag.clone());
    }
    result.system_tags.sort();
    result.system_tags.dedup();
    result.topic_tags = result
        .topic_tags
        .drain(..)
        .map(|tag| tag.trim().to_lowercase())
        .filter(|tag| !tag.is_empty())
        .collect();
    result.topic_tags.sort();
    result.topic_tags.dedup();
    if let Some(details) = result.details.as_object_mut() {
        for redundant_key in ["primary_tag", "system_tags", "topic_tags", "storage_action"] {
            details.remove(redundant_key);
        }
    }
}

fn legacy_category(primary_tag: &str) -> &'static str {
    match primary_tag {
        "idea" => "inspiration",
        "reflection" | "lesson" => "emotions",
        "question" => "study",
        _ => "life",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{config, mock_server};

    fn response(action: &str) -> String {
        let content = serde_json::json!({
            "storage_action": action,
            "confidence": 0.9,
            "reason_code": "personal_event",
            "primary_tag": "activity",
            "system_tags": ["activity", "work"],
            "topic_tags": ["Testing"],
            "details": {"system_tags":["activity"],"project":"Daily Agent"},
            "summary": "Finished a task",
            "entities": [],
            "sentiment": "positive",
            "importance": 4
        })
        .to_string();
        serde_json::json!({"message":{"content":content}}).to_string()
    }

    #[tokio::test]
    async fn classifies_with_local_structured_output() {
        let (url, handle) = mock_server(vec![response("store")]).await;
        let config = config("sqlite::memory:".into(), url.clone(), None);
        let decision = LocalClassifier::from_test_url(url, &config)
            .decide("I finished a task")
            .await
            .unwrap();
        assert_eq!(decision.action, StorageAction::Store);
        assert_eq!(decision.analysis.primary_tag, "activity");
        assert_eq!(decision.analysis.topic_tags, ["testing"]);
        assert_eq!(
            decision.analysis.details,
            serde_json::json!({"project":"Daily Agent"})
        );
        let requests = handle.await.unwrap();
        assert!(requests[0].contains("test-local-model"));
        assert!(requests[0].contains("storage_action"));
    }

    #[tokio::test]
    async fn rejects_invalid_output() {
        for body in [
            serde_json::json!({"message":{"content":"not-json"}}).to_string(),
            response("invalid"),
        ] {
            let (url, handle) = mock_server(vec![body]).await;
            let config = config("sqlite::memory:".into(), url.clone(), None);
            assert!(
                LocalClassifier::from_test_url(url, &config)
                    .decide("text")
                    .await
                    .is_err()
            );
            handle.await.unwrap();
        }
    }
}
