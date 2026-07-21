use anyhow::{Context, Result};
use secrecy::SecretString;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub local_user: String,
    pub glm_api_key: Option<SecretString>,
    pub glm_base_url: String,
    pub glm_model: String,
    pub telegram_token: Option<SecretString>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let db = std::env::var("DAILY_AGENT_DB").unwrap_or_else(|_| "./data/daily-agent.db".into());
        if let Some(parent) = std::path::Path::new(&db).parent() {
            std::fs::create_dir_all(parent).context("无法创建数据库目录")?;
        }
        Ok(Self {
            database_url: format!("sqlite://{db}"),
            local_user: std::env::var("DAILY_AGENT_USER").unwrap_or_else(|_| "default".into()),
            glm_api_key: std::env::var("GLM_API_KEY").ok().map(Into::into),
            glm_base_url: std::env::var("GLM_BASE_URL")
                .unwrap_or_else(|_| "https://open.bigmodel.cn/api/paas/v4".into()),
            glm_model: std::env::var("GLM_MODEL").unwrap_or_else(|_| "glm-5.2".into()),
            telegram_token: std::env::var("TELOXIDE_TOKEN").ok().map(Into::into),
        })
    }
}
