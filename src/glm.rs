use crate::{
    config::Config,
    models::{Analysis, ConnectionAnalysis},
    prompts::PromptStore,
};
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

    pub async fn analyze(&self, text: &str) -> Result<Analysis> {
        let analysis: Analysis = self.json(self.prompts.classify(), text).await?;
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
            .contains(&analysis.category.as_str()),
            "GLM returned an invalid category code"
        );
        anyhow::ensure!(
            ["positive", "neutral", "negative", "mixed"].contains(&analysis.sentiment.as_str()),
            "GLM returned an invalid sentiment code"
        );
        anyhow::ensure!(
            (1..=5).contains(&analysis.importance),
            "GLM returned an invalid importance value"
        );
        Ok(analysis)
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

    fn valid_analysis() -> String {
        serde_json::json!({
            "category":"work",
            "summary":"Worked on tests",
            "topics":["testing"],
            "entities":[],
            "sentiment":"positive",
            "importance":4
        })
        .to_string()
    }

    #[tokio::test]
    async fn parses_and_validates_analysis() {
        let (client, handle) = client_with(&valid_analysis()).await;
        let result = client.analyze("a log").await.unwrap();
        assert_eq!(result.category, "work");
        let requests = handle.await.unwrap();
        assert!(requests[0].contains("test-model"));
        assert!(requests[0].contains("a log"));
    }

    #[tokio::test]
    async fn rejects_invalid_analysis_codes_and_values() {
        for (field, value) in [
            ("category", serde_json::json!("invalid")),
            ("sentiment", serde_json::json!("invalid")),
            ("importance", serde_json::json!(9)),
        ] {
            let mut analysis: serde_json::Value = serde_json::from_str(&valid_analysis()).unwrap();
            analysis[field] = value;
            let (client, handle) = client_with(&analysis.to_string()).await;
            assert!(client.analyze("log").await.is_err());
            handle.await.unwrap();
        }
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
                .analyze("log")
                .await
                .is_err()
        );
        handle.await.unwrap();

        let (client, handle) = client_with("not json").await;
        assert!(client.analyze("log").await.is_err());
        handle.await.unwrap();
    }
}
