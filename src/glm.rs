use crate::{config::Config, models::ConnectionAnalysis, prompts::PromptStore};
use anyhow::{Context, Result};
use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Clone)]
pub struct GlmClient {
    http: Client,
    api_key: SecretString,
    base_url: String,
    model: String,
    prompts: PromptStore,
}

#[derive(Serialize)]
struct Request<'a> {
    model: &'a str,
    messages: Vec<Message<'a>>,
    temperature: f32,
    response_format: ResponseFormat,
}
#[derive(Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}
#[derive(Serialize)]
struct ResponseFormat {
    r#type: &'static str,
}
#[derive(Deserialize)]
struct Response {
    choices: Vec<Choice>,
}
#[derive(Deserialize)]
struct Choice {
    message: ResponseMessage,
}
#[derive(Deserialize)]
struct ResponseMessage {
    content: String,
}

impl GlmClient {
    pub fn from_config(config: &Config) -> Result<Self> {
        let api_key = config.glm_api_key.clone().context("missing GLM_API_KEY")?;
        Ok(Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()?,
            api_key,
            base_url: config.glm_base_url.trim_end_matches('/').into(),
            model: config.glm_model.clone(),
            prompts: config.prompts.clone(),
        })
    }

    async fn json<T: DeserializeOwned>(&self, system: &str, input: &str) -> Result<T> {
        let body = Request {
            model: &self.model,
            messages: vec![
                Message {
                    role: "system",
                    content: system,
                },
                Message {
                    role: "user",
                    content: input,
                },
            ],
            temperature: 0.2,
            response_format: ResponseFormat {
                r#type: "json_object",
            },
        };
        let response = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(self.api_key.expose_secret())
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json::<Response>()
            .await?;
        let content = response
            .choices
            .first()
            .context("GLM returned empty choices")?
            .message
            .content
            .trim();
        serde_json::from_str(content).with_context(|| "GLM did not return valid JSON")
    }

    pub async fn connections(&self, input: &str) -> Result<ConnectionAnalysis> {
        let mut analysis: ConnectionAnalysis = self.json(self.prompts.connections(), input).await?;
        analysis.connections.retain(|connection| {
            [
                "shared_topic",
                "person_link",
                "time_evolution",
                "goal_progress",
                "tension",
                "causal_clue",
            ]
            .contains(&connection.kind.as_str())
        });
        Ok(analysis)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{config as test_config, glm_response, mock_server};

    async fn client_with(content: &str) -> (GlmClient, tokio::task::JoinHandle<Vec<String>>) {
        let (url, handle) = mock_server(vec![glm_response(content)]).await;
        let config = test_config("sqlite::memory:".into(), url, Some("key"));
        (GlmClient::from_config(&config).unwrap(), handle)
    }

    #[tokio::test]
    async fn filters_invalid_connection_kinds() {
        let content = serde_json::json!({
            "overview":"overview",
            "connections":[
                {"kind":"shared_topic","description":"valid","confidence":0.8,"source_log_ids":["a","b"]},
                {"kind":"invented","description":"invalid","confidence":0.8,"source_log_ids":["a","b"]}
            ]
        })
        .to_string();
        let (client, handle) = client_with(&content).await;
        let result = client.connections("[]").await.unwrap();
        assert_eq!(result.connections.len(), 1);
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn handles_missing_key_empty_choices_and_invalid_json() {
        let config = test_config("sqlite::memory:".into(), "http://localhost".into(), None);
        assert!(GlmClient::from_config(&config).is_err());

        let (url, handle) = mock_server(vec!["{\"choices\":[]}".into()]).await;
        let config = test_config("sqlite::memory:".into(), url, Some("key"));
        assert!(
            GlmClient::from_config(&config)
                .unwrap()
                .connections("[]")
                .await
                .is_err()
        );
        handle.await.unwrap();

        let (client, handle) = client_with("not json").await;
        assert!(client.connections("[]").await.is_err());
        handle.await.unwrap();
    }
}
