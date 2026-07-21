use crate::{
    config::Config,
    models::{Analysis, ConnectionAnalysis},
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
        let api_key = config.glm_api_key.clone().context("缺少 GLM_API_KEY")?;
        Ok(Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()?,
            api_key,
            base_url: config.glm_base_url.trim_end_matches('/').into(),
            model: config.glm_model.clone(),
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
            .context("GLM 返回了空 choices")?
            .message
            .content
            .trim();
        serde_json::from_str(content).with_context(|| "GLM 未返回有效的 JSON")
    }

    pub async fn analyze(&self, text: &str) -> Result<Analysis> {
        self.json("你是私人日志分类器。只根据输入，不虚构。返回 JSON：category 必须是工作、学习、健康、关系、财务、灵感、情绪、生活、其他之一；summary 是简洁中文；topics 是短词数组；entities 是 {kind,name} 数组，仅提取明确出现的人物、项目、地点；sentiment 是积极、中性、消极、复杂之一；importance 是 1 到 5 的整数。不要输出 JSON 以外的内容。", text).await
    }

    pub async fn connections(&self, input: &str) -> Result<ConnectionAnalysis> {
        self.json("你是谨慎的私人日志关联分析器。返回 JSON：overview 是简洁总结；connections 是数组，每项包含 kind、description、confidence(0到1)、source_log_ids(至少两个输入中真实存在的ID)。kind 仅限共同主题、人物关联、时间演变、目标进展、潜在矛盾、因果线索。没有充分证据时返回空数组，因果只能表述为可能。不要输出 JSON 以外的内容。", input).await
    }
}
