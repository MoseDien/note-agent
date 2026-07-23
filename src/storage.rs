use crate::{
    config::Config,
    models::{StorageAction, StorageDecision},
};
use anyhow::{Context, Result};
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use serde::Deserialize;
use std::{
    cmp::Ordering,
    fs,
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Deserialize)]
struct Examples {
    store: Vec<String>,
    ignore: Vec<String>,
}

struct LabeledEmbedding {
    action: StorageAction,
    embedding: Vec<f32>,
}

enum GateInner {
    Disabled,
    #[cfg(test)]
    Fixed(StorageDecision),
    Local {
        model: Box<Mutex<TextEmbedding>>,
        examples: Vec<LabeledEmbedding>,
        minimum_similarity: f32,
        minimum_margin: f32,
        top_k: usize,
    },
}

#[derive(Clone)]
pub struct StorageGate {
    inner: Arc<GateInner>,
}

impl StorageGate {
    pub async fn from_config(config: &Config) -> Result<Self> {
        if !config.storage_enabled {
            return Ok(Self::disabled());
        }
        let examples_path = config.storage_examples_path.clone();
        let cache_dir = config.storage_model_cache.clone();
        let minimum_similarity = config.storage_min_similarity;
        let minimum_margin = config.storage_min_margin;
        let top_k = config.storage_top_k;
        tokio::task::spawn_blocking(move || {
            Self::load(
                &examples_path,
                &cache_dir,
                minimum_similarity,
                minimum_margin,
                top_k,
            )
        })
        .await
        .context("storage classifier initialization task failed")?
    }

    pub fn disabled() -> Self {
        Self {
            inner: Arc::new(GateInner::Disabled),
        }
    }

    #[cfg(test)]
    pub fn from_test_decision(decision: StorageDecision) -> Self {
        Self {
            inner: Arc::new(GateInner::Fixed(decision)),
        }
    }

    fn load(
        examples_path: &Path,
        cache_dir: &Path,
        minimum_similarity: f32,
        minimum_margin: f32,
        top_k: usize,
    ) -> Result<Self> {
        let examples: Examples =
            serde_json::from_str(&fs::read_to_string(examples_path).with_context(|| {
                format!(
                    "failed to read storage examples: {}",
                    examples_path.display()
                )
            })?)
            .context("invalid storage examples JSON")?;
        anyhow::ensure!(!examples.store.is_empty(), "store examples cannot be empty");
        anyhow::ensure!(
            !examples.ignore.is_empty(),
            "ignore examples cannot be empty"
        );
        anyhow::ensure!(top_k > 0, "storage top_k must be greater than zero");

        let mut model = TextEmbedding::try_new(
            TextInitOptions::new(EmbeddingModel::MultilingualE5Small)
                .with_cache_dir(cache_dir.to_path_buf())
                .with_show_download_progress(true),
        )
        .context("failed to load multilingual-e5-small")?;

        let mut texts = Vec::with_capacity(examples.store.len() + examples.ignore.len());
        let mut labels = Vec::with_capacity(texts.capacity());
        for text in examples.store {
            texts.push(format!("passage: {text}"));
            labels.push(StorageAction::Store);
        }
        for text in examples.ignore {
            texts.push(format!("passage: {text}"));
            labels.push(StorageAction::Ignore);
        }
        let vectors = model.embed(&texts, Some(16))?;
        let examples = labels
            .into_iter()
            .zip(vectors)
            .map(|(action, embedding)| LabeledEmbedding { action, embedding })
            .collect();
        Ok(Self {
            inner: Arc::new(GateInner::Local {
                model: Box::new(Mutex::new(model)),
                examples,
                minimum_similarity,
                minimum_margin,
                top_k,
            }),
        })
    }

    pub async fn decide(&self, text: &str) -> Result<StorageDecision> {
        let inner = self.inner.clone();
        let text = text.to_owned();
        tokio::task::spawn_blocking(move || match inner.as_ref() {
            GateInner::Disabled => Ok(StorageDecision {
                action: StorageAction::Store,
                store_score: 1.0,
                ignore_score: 0.0,
            }),
            #[cfg(test)]
            GateInner::Fixed(decision) => Ok(decision.clone()),
            GateInner::Local {
                model,
                examples,
                minimum_similarity,
                minimum_margin,
                top_k,
            } => {
                let input = [format!("query: {text}")];
                let embedding = model
                    .lock()
                    .map_err(|_| anyhow::anyhow!("storage model lock poisoned"))?
                    .embed(&input, Some(1))?
                    .pop()
                    .context("storage model returned no embedding")?;
                Ok(decide_from_embedding(
                    &embedding,
                    examples,
                    *minimum_similarity,
                    *minimum_margin,
                    *top_k,
                ))
            }
        })
        .await
        .context("storage classifier task failed")?
    }
}

fn decide_from_embedding(
    input: &[f32],
    examples: &[LabeledEmbedding],
    minimum_similarity: f32,
    minimum_margin: f32,
    top_k: usize,
) -> StorageDecision {
    let store_score = top_k_average(input, examples, StorageAction::Store, top_k);
    let ignore_score = top_k_average(input, examples, StorageAction::Ignore, top_k);
    let best = store_score.max(ignore_score);
    let margin = (store_score - ignore_score).abs();
    let action = if best < minimum_similarity || margin < minimum_margin {
        StorageAction::Ask
    } else if store_score > ignore_score {
        StorageAction::Store
    } else {
        StorageAction::Ignore
    };
    StorageDecision {
        action,
        store_score,
        ignore_score,
    }
}

fn top_k_average(
    input: &[f32],
    examples: &[LabeledEmbedding],
    action: StorageAction,
    top_k: usize,
) -> f32 {
    let mut scores: Vec<f32> = examples
        .iter()
        .filter(|example| example.action == action)
        .map(|example| cosine_similarity(input, &example.embedding))
        .collect();
    scores.sort_unstable_by(|a, b| b.partial_cmp(a).unwrap_or(Ordering::Equal));
    let count = scores.len().min(top_k);
    if count == 0 {
        return 0.0;
    }
    scores[..count].iter().sum::<f32>() / count as f32
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    let dot = left.iter().zip(right).map(|(a, b)| a * b).sum::<f32>();
    let left_norm = left.iter().map(|v| v * v).sum::<f32>().sqrt();
    let right_norm = right.iter().map(|v| v * v).sum::<f32>().sqrt();
    if left_norm == 0.0 || right_norm == 0.0 {
        0.0
    } else {
        dot / (left_norm * right_norm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example(action: StorageAction, embedding: &[f32]) -> LabeledEmbedding {
        LabeledEmbedding {
            action,
            embedding: embedding.to_vec(),
        }
    }

    #[test]
    fn chooses_store_ignore_or_ask_from_similarity() {
        let examples = vec![
            example(StorageAction::Store, &[1.0, 0.0]),
            example(StorageAction::Store, &[0.9, 0.1]),
            example(StorageAction::Ignore, &[0.0, 1.0]),
            example(StorageAction::Ignore, &[0.1, 0.9]),
        ];
        assert_eq!(
            decide_from_embedding(&[1.0, 0.0], &examples, 0.7, 0.1, 2).action,
            StorageAction::Store
        );
        assert_eq!(
            decide_from_embedding(&[0.0, 1.0], &examples, 0.7, 0.1, 2).action,
            StorageAction::Ignore
        );
        assert_eq!(
            decide_from_embedding(&[0.7, 0.7], &examples, 0.7, 0.1, 2).action,
            StorageAction::Ask
        );
        assert_eq!(
            decide_from_embedding(&[-1.0, 0.0], &examples, 0.7, 0.1, 2).action,
            StorageAction::Ask
        );
    }

    #[test]
    fn cosine_and_top_k_handle_edge_cases() {
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 2.0]), 0.0);
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
        assert!((cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert_eq!(top_k_average(&[1.0], &[], StorageAction::Store, 3), 0.0);
    }

    #[test]
    fn rejects_missing_or_invalid_example_files_before_loading_model() {
        let temp = tempfile::tempdir().unwrap();
        let cache = temp.path().join("cache");
        let missing = temp.path().join("missing.json");
        assert!(StorageGate::load(&missing, &cache, 0.75, 0.03, 3).is_err());

        let invalid = temp.path().join("invalid.json");
        fs::write(&invalid, "not json").unwrap();
        assert!(StorageGate::load(&invalid, &cache, 0.75, 0.03, 3).is_err());

        let empty_store = temp.path().join("empty-store.json");
        fs::write(&empty_store, r#"{"store":[],"ignore":["hello"]}"#).unwrap();
        assert!(StorageGate::load(&empty_store, &cache, 0.75, 0.03, 3).is_err());

        let empty_ignore = temp.path().join("empty-ignore.json");
        fs::write(&empty_ignore, r#"{"store":["hello"],"ignore":[]}"#).unwrap();
        assert!(StorageGate::load(&empty_ignore, &cache, 0.75, 0.03, 3).is_err());

        let zero_top_k = temp.path().join("zero-top-k.json");
        fs::write(
            &zero_top_k,
            r#"{"store":["remember this"],"ignore":["ignore this"]}"#,
        )
        .unwrap();
        assert!(StorageGate::load(&zero_top_k, &cache, 0.75, 0.03, 0).is_err());
    }

    #[tokio::test]
    async fn disabled_and_fixed_gates_return_without_a_model() {
        assert_eq!(
            StorageGate::disabled()
                .decide("anything")
                .await
                .unwrap()
                .action,
            StorageAction::Store
        );
        let expected = StorageDecision {
            action: StorageAction::Ignore,
            store_score: 0.2,
            ignore_score: 0.9,
        };
        let actual = StorageGate::from_test_decision(expected.clone())
            .decide("anything")
            .await
            .unwrap();
        assert_eq!(actual.action, expected.action);
    }

    #[test]
    fn example_dataset_has_five_hundred_items_per_class() {
        let examples: Examples =
            serde_json::from_str(&fs::read_to_string("./resources/storage-examples.json").unwrap())
                .unwrap();
        assert_eq!(examples.store.len(), 500);
        assert_eq!(examples.ignore.len(), 500);
        assert_eq!(
            examples
                .store
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            500
        );
        assert_eq!(
            examples
                .ignore
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            500
        );
    }
}
