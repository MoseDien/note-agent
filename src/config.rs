use crate::{i18n::I18n, prompts::PromptStore};
use anyhow::{Context, Result};
use secrecy::SecretString;
use std::path::PathBuf;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub local_user: String,
    pub glm_api_key: Option<SecretString>,
    pub glm_base_url: String,
    pub glm_model: String,
    pub telegram_token: Option<SecretString>,
    pub i18n: I18n,
    pub prompts: PromptStore,
    pub storage_enabled: bool,
    pub storage_examples_path: PathBuf,
    pub storage_model_cache: PathBuf,
    pub storage_min_similarity: f32,
    pub storage_min_margin: f32,
    pub storage_top_k: usize,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let db = std::env::var("DAILY_AGENT_DB").unwrap_or_else(|_| "./data/daily-agent.db".into());
        if let Some(parent) = std::path::Path::new(&db).parent() {
            std::fs::create_dir_all(parent).context("failed to create database directory")?;
        }
        let locale = std::env::var("DAILY_AGENT_LOCALE").unwrap_or_else(|_| "zh-CN".into());
        let resources =
            std::env::var("DAILY_AGENT_RESOURCES").unwrap_or_else(|_| "./resources".into());
        let i18n = I18n::load(&resources, &locale)?;
        let prompts = PromptStore::load(&resources, &locale)?;
        let storage_enabled = parse("DAILY_AGENT_STORAGE_ENABLED", true)?;
        let storage_examples_path = std::env::var("DAILY_AGENT_STORAGE_EXAMPLES")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(&resources).join("storage-examples.json"));
        Ok(Self {
            database_url: format!("sqlite://{db}"),
            local_user: std::env::var("DAILY_AGENT_USER").unwrap_or_else(|_| "default".into()),
            glm_api_key: std::env::var("GLM_API_KEY").ok().map(Into::into),
            glm_base_url: std::env::var("GLM_BASE_URL")
                .unwrap_or_else(|_| "https://open.bigmodel.cn/api/paas/v4".into()),
            glm_model: std::env::var("GLM_MODEL").unwrap_or_else(|_| "glm-5.2".into()),
            telegram_token: std::env::var("TELOXIDE_TOKEN").ok().map(Into::into),
            i18n,
            prompts,
            storage_enabled,
            storage_examples_path,
            storage_model_cache: std::env::var("FASTEMBED_CACHE_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| ".fastembed_cache".into()),
            storage_min_similarity: parse("DAILY_AGENT_STORAGE_MIN_SIMILARITY", 0.75)?,
            storage_min_margin: parse("DAILY_AGENT_STORAGE_MIN_MARGIN", 0.03)?,
            storage_top_k: parse("DAILY_AGENT_STORAGE_TOP_K", 3)?,
        })
    }
}

fn parse<T>(name: &str, default: T) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match std::env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|error| anyhow::anyhow!("invalid {name}: {error}")),
        Err(_) => Ok(default),
    }
}
