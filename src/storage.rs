use crate::{
    config::Config,
    models::{Analysis, StorageAction, StorageDecision},
};
use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;

#[derive(Clone)]
pub struct StorageGate {
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
    category: String,
    summary: String,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default)]
    entities: Vec<crate::models::EntityMention>,
    sentiment: String,
    importance: u8,
}

impl StorageGate {
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
                "category": {"type":"string","enum":["work","study","health","relationships",
                    "finance","inspiration","emotions","life","other"]},
                "summary": {"type":"string","maxLength":200},
                "topics": {"type":"array","maxItems":8,"items":{"type":"string","maxLength":60}},
                "entities": {"type":"array","items":{"type":"object","properties":{
                    "kind":{"type":"string","maxLength":40},"name":{"type":"string","maxLength":80}
                },"required":["kind","name"]},"maxItems":8},
                "sentiment": {"type":"string","enum":["positive","neutral","negative","mixed"]},
                "importance": {"type":"integer","minimum":1,"maximum":5}
            },
            "required":["storage_action","confidence","reason_code","category","summary",
                "topics","entities","sentiment","importance"]
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
        let result: LocalResult = serde_json::from_str(response.message.content.trim())
            .context("local model did not return valid JSON")?;
        validate(&result)?;
        Ok(StorageDecision {
            action: result.storage_action,
            confidence: result.confidence,
            reason_code: result.reason_code,
            analysis: Analysis {
                category: result.category,
                summary: result.summary,
                topics: result.topics,
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
    anyhow::ensure!(
        [
            "work",
            "study",
            "health",
            "relationships",
            "finance",
            "inspiration",
            "emotions",
            "life",
            "other"
        ]
        .contains(&result.category.as_str()),
        "invalid local category"
    );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{config, mock_server};

    fn response(action: &str) -> String {
        let content = serde_json::json!({
            "storage_action": action,
            "confidence": 0.9,
            "reason_code": "personal_event",
            "category": "work",
            "summary": "Finished a task",
            "topics": ["testing"],
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
        let decision = StorageGate::from_test_url(url, &config)
            .decide("I finished a task")
            .await
            .unwrap();
        assert_eq!(decision.action, StorageAction::Store);
        assert_eq!(decision.analysis.category, "work");
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
                StorageGate::from_test_url(url, &config)
                    .decide("text")
                    .await
                    .is_err()
            );
            handle.await.unwrap();
        }
    }
}
