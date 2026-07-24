use crate::{
    config::Config,
    models::{StorageAction, StorageDecision},
};
use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;

#[derive(Clone)]
pub struct LocalStorageGate {
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
#[serde(deny_unknown_fields)]
struct LocalResult {
    storage_action: StorageAction,
}

impl LocalStorageGate {
    pub async fn from_config(config: &Config) -> Result<Self> {
        Ok(Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(
                    config.local_llm_timeout_seconds,
                ))
                .build()?,
            url: config.local_llm_url.trim_end_matches('/').into(),
            model: config.local_llm_model.clone(),
            prompt: config.prompts.storage_decision().into(),
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
            prompt: config.prompts.storage_decision().into(),
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
            "additionalProperties": false,
            "properties": {
                "storage_action": {"type":"string","enum":["store","ignore","ask"]}
            },
            "required":["storage_action"]
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
            "options": {"temperature":0,"num_predict":96,"num_ctx":2048}
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
        Ok(StorageDecision {
            action: result.storage_action,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{config, mock_server};

    fn response(action: &str) -> String {
        let content = serde_json::json!({
            "storage_action": action
        })
        .to_string();
        serde_json::json!({"message":{"content":content}}).to_string()
    }

    #[tokio::test]
    async fn decides_storage_with_structured_output() {
        let (url, handle) = mock_server(vec![response("store")]).await;
        let config = config("sqlite::memory:".into(), url.clone(), None);
        let decision = LocalStorageGate::from_test_url(url, &config)
            .decide("I finished a task")
            .await
            .unwrap();
        assert_eq!(decision.action, StorageAction::Store);
        let requests = handle.await.unwrap();
        assert!(requests[0].contains("test-local-model"));
        assert!(requests[0].contains("storage_action"));
    }

    #[tokio::test]
    async fn rejects_invalid_output() {
        for body in [
            serde_json::json!({"message":{"content":"not-json"}}).to_string(),
            response("invalid"),
            serde_json::json!({"message":{"content":serde_json::json!({
                "storage_action":"store",
                "summary":"must not be accepted"
            }).to_string()}})
            .to_string(),
        ] {
            let (url, handle) = mock_server(vec![body]).await;
            let config = config("sqlite::memory:".into(), url.clone(), None);
            assert!(
                LocalStorageGate::from_test_url(url, &config)
                    .decide("text")
                    .await
                    .is_err()
            );
            handle.await.unwrap();
        }
    }
}
