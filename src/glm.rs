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
