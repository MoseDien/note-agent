use anyhow::{Context, Result};
use std::{collections::HashMap, fs, path::Path, sync::Arc};

const SUPPORTED_LOCALES: &[&str] = &["zh-CN", "en-US"];

#[derive(Clone)]
pub struct I18n {
    messages: Arc<HashMap<String, String>>,
}

impl I18n {
    pub fn load(resources: impl AsRef<Path>, locale: &str) -> Result<Self> {
        anyhow::ensure!(
            SUPPORTED_LOCALES.contains(&locale),
            "unsupported DAILY_AGENT_LOCALE: {locale}; expected zh-CN or en-US"
        );
        let path = resources
            .as_ref()
            .join("locales")
            .join(format!("{locale}.json"));
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("failed to read locale file: {}", path.display()))?;
        let messages = serde_json::from_str(&contents)
            .with_context(|| format!("invalid locale JSON: {}", path.display()))?;
        Ok(Self {
            messages: Arc::new(messages),
        })
    }

    pub fn text(&self, key: &str) -> String {
        self.messages
            .get(key)
            .cloned()
            .unwrap_or_else(|| format!("[{key}]"))
    }

    pub fn format(&self, key: &str, values: &[(&str, &str)]) -> String {
        values.iter().fold(self.text(key), |text, (name, value)| {
            text.replace(&format!("{{{name}}}"), value)
        })
    }

    pub fn category(&self, value: Option<&str>) -> String {
        value
            .map(|code| {
                let key = format!("category.{code}");
                self.messages
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| code.to_owned())
            })
            .unwrap_or_else(|| self.text("category.pending"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_both_supported_languages() {
        let zh = I18n::load("./resources", "zh-CN").unwrap();
        let en = I18n::load("./resources", "en-US").unwrap();
        assert_eq!(zh.category(Some("work")), "工作");
        assert_eq!(en.category(Some("work")), "Work");
        assert_eq!(
            en.format("terminal.deleted", &[("id", "abc")]),
            "Deleted abc"
        );
    }

    #[test]
    fn rejects_unsupported_language() {
        assert!(I18n::load("./resources", "fr-FR").is_err());
    }
}
