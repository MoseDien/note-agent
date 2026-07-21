use anyhow::{Context, Result};
use std::{fs, path::Path, sync::Arc};

#[derive(Clone)]
pub struct PromptStore {
    classify: Arc<String>,
    connections: Arc<String>,
}

impl PromptStore {
    pub fn load(resources: impl AsRef<Path>, locale: &str) -> Result<Self> {
        let directory = resources.as_ref().join("prompts").join(locale);
        Ok(Self {
            classify: Arc::new(read(&directory.join("classify.system.md"))?),
            connections: Arc::new(read(&directory.join("connections.system.md"))?),
        })
    }

    pub fn classify(&self) -> &str {
        &self.classify
    }

    pub fn connections(&self) -> &str {
        &self.connections
    }
}

fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path)
        .with_context(|| format!("failed to read prompt file: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_external_prompts_for_both_languages() {
        for locale in ["zh-CN", "en-US"] {
            let prompts = PromptStore::load("./resources", locale).unwrap();
            assert!(prompts.classify().contains("work"));
            assert!(prompts.connections().contains("shared_topic"));
        }
    }
}
