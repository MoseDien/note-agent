use crate::models::{Analysis, Connection, Log, User};
use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use sqlx::{Row, SqlitePool, sqlite::SqliteConnectOptions};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

impl Store {
    pub async fn connect(url: &str) -> Result<Self> {
        let options = SqliteConnectOptions::from_str(url)?
            .create_if_missing(true)
            .foreign_keys(true);
        Ok(Self {
            pool: SqlitePool::connect_with(options).await?,
        })
    }

    pub async fn migrate(&self) -> Result<()> {
        for sql in [
            "CREATE TABLE IF NOT EXISTS users (id TEXT PRIMARY KEY, local_name TEXT NOT NULL UNIQUE, created_at TEXT NOT NULL)",
            "CREATE TABLE IF NOT EXISTS channel_identities (user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE, channel TEXT NOT NULL, external_id TEXT NOT NULL, UNIQUE(channel, external_id))",
            "CREATE TABLE IF NOT EXISTS logs (id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE, channel TEXT NOT NULL, text TEXT NOT NULL, privacy_level TEXT NOT NULL DEFAULT 'normal', analysis_status TEXT NOT NULL DEFAULT 'pending', category TEXT, summary TEXT, topics_json TEXT, primary_tag TEXT, system_tags_json TEXT NOT NULL DEFAULT '[]', topic_tags_json TEXT NOT NULL DEFAULT '[]', details_json TEXT NOT NULL DEFAULT '{}', tag_schema_version INTEGER NOT NULL DEFAULT 1, sentiment TEXT, importance INTEGER, created_at TEXT NOT NULL)",
            "CREATE INDEX IF NOT EXISTS logs_user_created ON logs(user_id, created_at DESC)",
            "CREATE VIRTUAL TABLE IF NOT EXISTS logs_fts USING fts5(log_id UNINDEXED, user_id UNINDEXED, text, summary, topics)",
            "CREATE TABLE IF NOT EXISTS entities (id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE, kind TEXT NOT NULL, name TEXT NOT NULL, UNIQUE(user_id, kind, name))",
            "CREATE TABLE IF NOT EXISTS entity_mentions (entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE, log_id TEXT NOT NULL REFERENCES logs(id) ON DELETE CASCADE, UNIQUE(entity_id, log_id))",
            "CREATE TABLE IF NOT EXISTS connections (id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE, kind TEXT NOT NULL, description TEXT NOT NULL, confidence REAL NOT NULL, created_at TEXT NOT NULL)",
            "CREATE TABLE IF NOT EXISTS connection_sources (connection_id TEXT NOT NULL REFERENCES connections(id) ON DELETE CASCADE, log_id TEXT NOT NULL REFERENCES logs(id) ON DELETE CASCADE, UNIQUE(connection_id, log_id))",
            "CREATE TABLE IF NOT EXISTS pairing_codes (code TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE, expires_at TEXT NOT NULL, used_at TEXT)",
        ] {
            sqlx::query(sql).execute(&self.pool).await?;
        }
        for (name, definition) in [
            ("primary_tag", "TEXT"),
            ("system_tags_json", "TEXT NOT NULL DEFAULT '[]'"),
            ("topic_tags_json", "TEXT NOT NULL DEFAULT '[]'"),
            ("details_json", "TEXT NOT NULL DEFAULT '{}'"),
            ("tag_schema_version", "INTEGER NOT NULL DEFAULT 0"),
        ] {
            self.ensure_log_column(name, definition).await?;
        }
        for (legacy, code) in [
            ("工作", "work"),
            ("学习", "study"),
            ("健康", "health"),
            ("关系", "relationships"),
            ("财务", "finance"),
            ("灵感", "inspiration"),
            ("情绪", "emotions"),
            ("生活", "life"),
            ("其他", "other"),
        ] {
            sqlx::query("UPDATE logs SET category=? WHERE category=?")
                .bind(code)
                .bind(legacy)
                .execute(&self.pool)
                .await?;
        }
        sqlx::query(
            "UPDATE logs SET
                primary_tag=CASE category
                    WHEN 'work' THEN 'activity' WHEN 'study' THEN 'experience'
                    WHEN 'health' THEN 'experience' WHEN 'relationships' THEN 'reflection'
                    WHEN 'finance' THEN 'activity' WHEN 'inspiration' THEN 'idea'
                    WHEN 'emotions' THEN 'reflection' WHEN 'life' THEN 'experience'
                    ELSE NULL END,
                system_tags_json=CASE category
                    WHEN 'work' THEN '[\"activity\",\"work\"]'
                    WHEN 'study' THEN '[\"experience\",\"learning\"]'
                    WHEN 'health' THEN '[\"experience\",\"health\"]'
                    WHEN 'relationships' THEN '[\"reflection\",\"relationship\"]'
                    WHEN 'finance' THEN '[\"activity\",\"finance\"]'
                    WHEN 'inspiration' THEN '[\"idea\"]'
                    WHEN 'emotions' THEN '[\"reflection\",\"wellbeing\",\"mood\"]'
                    WHEN 'life' THEN '[\"experience\",\"self\"]'
                    ELSE '[]' END,
                topic_tags_json=COALESCE(topics_json,'[]'),
                tag_schema_version=1
             WHERE tag_schema_version=0",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn ensure_log_column(&self, name: &str, definition: &str) -> Result<()> {
        let columns = sqlx::query("PRAGMA table_info(logs)")
            .fetch_all(&self.pool)
            .await?;
        if columns
            .iter()
            .any(|column| column.get::<String, _>("name") == name)
        {
            return Ok(());
        }
        sqlx::query(&format!("ALTER TABLE logs ADD COLUMN {name} {definition}"))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn ensure_local_user(&self, name: &str) -> Result<User> {
        if let Some(user) = sqlx::query_as::<_, User>("SELECT * FROM users WHERE local_name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?
        {
            return Ok(user);
        }
        let user = User {
            id: Uuid::new_v4().to_string(),
            local_name: name.to_owned(),
            created_at: Utc::now().to_rfc3339(),
        };
        sqlx::query("INSERT INTO users(id, local_name, created_at) VALUES (?, ?, ?)")
            .bind(&user.id)
            .bind(&user.local_name)
            .bind(&user.created_at)
            .execute(&self.pool)
            .await?;
        Ok(user)
    }

    pub async fn insert_log(
        &self,
        user_id: &str,
        channel: &str,
        text: &str,
        privacy_level: &str,
    ) -> Result<Log> {
        let log = Log {
            id: Uuid::new_v4().to_string(),
            user_id: user_id.into(),
            channel: channel.into(),
            text: text.into(),
            privacy_level: privacy_level.into(),
            analysis_status: if privacy_level == "no_upload" {
                "not_requested".into()
            } else {
                "pending".into()
            },
            category: None,
            summary: None,
            topics_json: None,
            primary_tag: None,
            system_tags_json: "[]".into(),
            topic_tags_json: "[]".into(),
            details_json: "{}".into(),
            tag_schema_version: 1,
            sentiment: None,
            importance: None,
            created_at: Utc::now().to_rfc3339(),
        };
        sqlx::query("INSERT INTO logs(id,user_id,channel,text,privacy_level,analysis_status,created_at) VALUES(?,?,?,?,?,?,?)")
            .bind(&log.id).bind(user_id).bind(channel).bind(text).bind(&log.privacy_level).bind(&log.analysis_status).bind(&log.created_at).execute(&self.pool).await?;
        Ok(log)
    }

    pub async fn save_analysis(&self, log_id: &str, analysis: &Analysis) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let topics = serde_json::to_string(&analysis.topics)?;
        let system_tags = serde_json::to_string(&analysis.system_tags)?;
        let topic_tags = serde_json::to_string(&analysis.topic_tags)?;
        let details = serde_json::to_string(&analysis.details)?;
        sqlx::query("UPDATE logs SET analysis_status='complete',category=?,summary=?,topics_json=?,primary_tag=?,system_tags_json=?,topic_tags_json=?,details_json=?,tag_schema_version=1,sentiment=?,importance=? WHERE id=?")
            .bind(&analysis.category).bind(&analysis.summary).bind(&topics)
            .bind(&analysis.primary_tag).bind(&system_tags).bind(&topic_tags).bind(&details)
            .bind(&analysis.sentiment).bind(analysis.importance as i64).bind(log_id).execute(&mut *tx).await?;
        let row = sqlx::query("SELECT user_id,text FROM logs WHERE id=?")
            .bind(log_id)
            .fetch_one(&mut *tx)
            .await?;
        let user_id: String = row.get("user_id");
        let text: String = row.get("text");
        sqlx::query("DELETE FROM logs_fts WHERE log_id=?")
            .bind(log_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO logs_fts(log_id,user_id,text,summary,topics) VALUES(?,?,?,?,?)")
            .bind(log_id)
            .bind(&user_id)
            .bind(text)
            .bind(&analysis.summary)
            .bind(
                analysis
                    .system_tags
                    .iter()
                    .chain(&analysis.topic_tags)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" "),
            )
            .execute(&mut *tx)
            .await?;
        for entity in &analysis.entities {
            let entity_id = Uuid::new_v4().to_string();
            sqlx::query("INSERT OR IGNORE INTO entities(id,user_id,kind,name) VALUES(?,?,?,?)")
                .bind(&entity_id)
                .bind(&user_id)
                .bind(&entity.kind)
                .bind(&entity.name)
                .execute(&mut *tx)
                .await?;
            let existing: String =
                sqlx::query_scalar("SELECT id FROM entities WHERE user_id=? AND kind=? AND name=?")
                    .bind(&user_id)
                    .bind(&entity.kind)
                    .bind(&entity.name)
                    .fetch_one(&mut *tx)
                    .await?;
            sqlx::query("INSERT OR IGNORE INTO entity_mentions(entity_id,log_id) VALUES(?,?)")
                .bind(existing)
                .bind(log_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub async fn mark_analysis_failed(&self, log_id: &str) -> Result<()> {
        sqlx::query("UPDATE logs SET analysis_status='failed' WHERE id=?")
            .bind(log_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    pub async fn get_log(&self, id: &str) -> Result<Log> {
        Ok(sqlx::query_as("SELECT * FROM logs WHERE id=?")
            .bind(id)
            .fetch_one(&self.pool)
            .await?)
    }
    pub async fn recent_logs(&self, user_id: &str, limit: u32) -> Result<Vec<Log>> {
        Ok(
            sqlx::query_as("SELECT * FROM logs WHERE user_id=? ORDER BY created_at DESC LIMIT ?")
                .bind(user_id)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?,
        )
    }
    pub async fn export_user(&self, user_id: &str) -> Result<Vec<Log>> {
        self.recent_logs(user_id, 100_000).await
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub async fn search_candidates(
        &self,
        user_id: &str,
        query: &str,
        exclude_id: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Log>> {
        let terms: Vec<String> = query
            .split(|c: char| c.is_whitespace() || "，。！？、；：,.!?;:".contains(c))
            .filter(|s| s.chars().count() >= 2)
            .take(8)
            .map(|s| format!("\"{}\"", s.replace('"', "")))
            .collect();
        if terms.is_empty() {
            return self.recent_logs(user_id, limit).await;
        }
        let fts_query = terms.join(" OR ");
        let rows = sqlx::query_as::<_, Log>("SELECT l.* FROM logs_fts f JOIN logs l ON l.id=f.log_id WHERE f.user_id=? AND logs_fts MATCH ? AND (? IS NULL OR l.id != ?) ORDER BY bm25(logs_fts), l.created_at DESC LIMIT ?")
            .bind(user_id).bind(fts_query).bind(exclude_id).bind(exclude_id).bind(limit).fetch_all(&self.pool).await;
        match rows {
            Ok(v) => Ok(v),
            Err(_) => self.recent_logs(user_id, limit).await,
        }
    }

    pub async fn save_connections(&self, user_id: &str, connections: &[Connection]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for item in connections {
            let id = Uuid::new_v4().to_string();
            sqlx::query("INSERT INTO connections(id,user_id,kind,description,confidence,created_at) VALUES(?,?,?,?,?,?)").bind(&id).bind(user_id).bind(&item.kind).bind(&item.description).bind(item.confidence).bind(Utc::now().to_rfc3339()).execute(&mut *tx).await?;
            for log_id in &item.source_log_ids {
                sqlx::query("INSERT OR IGNORE INTO connection_sources(connection_id,log_id) SELECT ?,id FROM logs WHERE id=? AND user_id=?").bind(&id).bind(log_id).bind(user_id).execute(&mut *tx).await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn delete_log(&self, user_id: &str, id: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM logs_fts WHERE log_id=? AND user_id=?")
            .bind(id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        let result = sqlx::query("DELETE FROM logs WHERE id=? AND user_id=?")
            .bind(id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM connections WHERE user_id=? AND id NOT IN (SELECT connection_id FROM connection_sources)").bind(user_id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn create_pairing_code(&self, user_id: &str) -> Result<String> {
        let code = Uuid::new_v4().simple().to_string()[..8].to_uppercase();
        sqlx::query("INSERT INTO pairing_codes(code,user_id,expires_at) VALUES(?,?,?)")
            .bind(&code)
            .bind(user_id)
            .bind((Utc::now() + Duration::minutes(10)).to_rfc3339())
            .execute(&self.pool)
            .await?;
        Ok(code)
    }

    pub async fn consume_pairing_code(&self, code: &str, telegram_id: i64) -> Result<User> {
        let mut tx = self.pool.begin().await?;
        let user_id: String = sqlx::query_scalar(
            "SELECT user_id FROM pairing_codes WHERE code=? AND used_at IS NULL AND expires_at>?",
        )
        .bind(code.to_uppercase())
        .bind(Utc::now().to_rfc3339())
        .fetch_optional(&mut *tx)
        .await?
        .context("pairing code is invalid or expired")?;
        sqlx::query("INSERT OR REPLACE INTO channel_identities(user_id,channel,external_id) VALUES(?,'telegram',?)").bind(&user_id).bind(telegram_id.to_string()).execute(&mut *tx).await?;
        sqlx::query("UPDATE pairing_codes SET used_at=? WHERE code=?")
            .bind(Utc::now().to_rfc3339())
            .bind(code.to_uppercase())
            .execute(&mut *tx)
            .await?;
        let user = sqlx::query_as("SELECT * FROM users WHERE id=?")
            .bind(&user_id)
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(user)
    }

    pub async fn telegram_user(&self, telegram_id: i64) -> Result<Option<User>> {
        Ok(sqlx::query_as("SELECT u.* FROM users u JOIN channel_identities c ON c.user_id=u.id WHERE c.channel='telegram' AND c.external_id=?").bind(telegram_id.to_string()).fetch_optional(&self.pool).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_store() -> (Store, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("daily-agent-{}.db", Uuid::new_v4()));
        let store = Store::connect(&format!("sqlite://{}", path.display()))
            .await
            .unwrap();
        store.migrate().await.unwrap();
        (store, path)
    }

    #[tokio::test]
    async fn isolates_users_and_deletes_only_owned_logs() {
        let (store, path) = test_store().await;
        let alice = store.ensure_local_user("alice").await.unwrap();
        let bob = store.ensure_local_user("bob").await.unwrap();
        let alice_log = store
            .insert_log(&alice.id, "terminal", "Alice secret", "normal")
            .await
            .unwrap();
        store
            .insert_log(&bob.id, "terminal", "Bob secret", "normal")
            .await
            .unwrap();

        assert_eq!(store.recent_logs(&alice.id, 10).await.unwrap().len(), 1);
        assert!(!store.delete_log(&bob.id, &alice_log.id).await.unwrap());
        assert_eq!(store.recent_logs(&alice.id, 10).await.unwrap().len(), 1);

        store.pool.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn pairing_code_is_single_use() {
        let (store, path) = test_store().await;
        let user = store.ensure_local_user("owner").await.unwrap();
        let code = store.create_pairing_code(&user.id).await.unwrap();
        let paired = store.consume_pairing_code(&code, 12345).await.unwrap();
        assert_eq!(paired.id, user.id);
        assert!(store.consume_pairing_code(&code, 67890).await.is_err());
        assert_eq!(
            store.telegram_user(12345).await.unwrap().unwrap().id,
            user.id
        );

        store.pool.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn persists_analysis_searches_and_cleans_derived_data() {
        let (store, path) = test_store().await;
        let user = store.ensure_local_user("analyst").await.unwrap();
        let first = store
            .insert_log(&user.id, "terminal", "Rust privacy project", "normal")
            .await
            .unwrap();
        let second = store
            .insert_log(&user.id, "telegram", "Rust memory design", "normal")
            .await
            .unwrap();
        let analysis = Analysis {
            primary_tag: "activity".into(),
            system_tags: vec!["activity".into(), "work".into()],
            topic_tags: vec!["rust".into(), "privacy".into()],
            details: serde_json::json!({"project":"Daily Agent"}),
            category: "work".into(),
            summary: "Rust project work".into(),
            topics: vec!["Rust".into(), "privacy".into()],
            entities: vec![crate::models::EntityMention {
                kind: "project".into(),
                name: "Daily Agent".into(),
            }],
            sentiment: "positive".into(),
            importance: 4,
        };
        store.save_analysis(&first.id, &analysis).await.unwrap();
        store.save_analysis(&second.id, &analysis).await.unwrap();

        let loaded = store.get_log(&first.id).await.unwrap();
        assert_eq!(loaded.category.as_deref(), Some("work"));
        assert_eq!(loaded.primary_tag.as_deref(), Some("activity"));
        assert_eq!(
            serde_json::from_str::<Vec<String>>(&loaded.system_tags_json).unwrap(),
            vec!["activity", "work"]
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&loaded.details_json).unwrap(),
            serde_json::json!({"project":"Daily Agent"})
        );
        assert_eq!(loaded.tag_schema_version, 1);
        assert_eq!(store.export_user(&user.id).await.unwrap().len(), 2);
        let matches = store
            .search_candidates(&user.id, "Rust privacy", Some(&second.id), 5)
            .await
            .unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, first.id);

        store
            .save_connections(
                &user.id,
                &[Connection {
                    kind: "shared_topic".into(),
                    description: "Both mention Rust".into(),
                    confidence: 0.9,
                    source_log_ids: vec![first.id.clone(), second.id.clone()],
                }],
            )
            .await
            .unwrap();
        assert!(store.delete_log(&user.id, &first.id).await.unwrap());
        assert!(store.get_log(&first.id).await.is_err());
        store.mark_analysis_failed(&second.id).await.unwrap();
        assert_eq!(
            store.get_log(&second.id).await.unwrap().analysis_status,
            "failed"
        );

        store.pool.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn migrates_legacy_categories_and_search_falls_back() {
        let (store, path) = test_store().await;
        let user = store.ensure_local_user("legacy").await.unwrap();
        let log = store
            .insert_log(&user.id, "terminal", "legacy", "normal")
            .await
            .unwrap();
        sqlx::query("UPDATE logs SET category='工作' WHERE id=?")
            .bind(&log.id)
            .execute(&store.pool)
            .await
            .unwrap();
        store.migrate().await.unwrap();
        assert_eq!(
            store.get_log(&log.id).await.unwrap().category.as_deref(),
            Some("work")
        );
        let fallback = store
            .search_candidates(&user.id, "x", None, 5)
            .await
            .unwrap();
        assert_eq!(fallback.len(), 1);

        store.pool.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn adds_and_backfills_tag_columns_on_a_legacy_database() {
        let path = std::env::temp_dir().join(format!("daily-agent-legacy-{}.db", Uuid::new_v4()));
        let store = Store::connect(&format!("sqlite://{}", path.display()))
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE users (
                id TEXT PRIMARY KEY,
                local_name TEXT NOT NULL UNIQUE,
                created_at TEXT NOT NULL
            )",
        )
        .execute(&store.pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE logs (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                channel TEXT NOT NULL,
                text TEXT NOT NULL,
                privacy_level TEXT NOT NULL DEFAULT 'normal',
                analysis_status TEXT NOT NULL DEFAULT 'pending',
                category TEXT,
                summary TEXT,
                topics_json TEXT,
                sentiment TEXT,
                importance INTEGER,
                created_at TEXT NOT NULL
            )",
        )
        .execute(&store.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO users(id,local_name,created_at) VALUES('u','legacy','2026-01-01T00:00:00Z')",
        )
        .execute(&store.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO logs(
                id,user_id,channel,text,category,topics_json,created_at
             ) VALUES('l','u','terminal','legacy','情绪','[\"sleep\"]','2026-01-01T00:00:00Z')",
        )
        .execute(&store.pool)
        .await
        .unwrap();

        store.migrate().await.unwrap();
        store.migrate().await.unwrap();
        let migrated = store.get_log("l").await.unwrap();
        assert_eq!(migrated.category.as_deref(), Some("emotions"));
        assert_eq!(migrated.primary_tag.as_deref(), Some("reflection"));
        assert_eq!(
            serde_json::from_str::<Vec<String>>(&migrated.system_tags_json).unwrap(),
            vec!["reflection", "wellbeing", "mood"]
        );
        assert_eq!(migrated.topic_tags_json, "[\"sleep\"]");
        assert_eq!(migrated.details_json, "{}");
        assert_eq!(migrated.tag_schema_version, 1);

        sqlx::query("UPDATE logs SET system_tags_json='[\"custom\"]' WHERE id='l'")
            .execute(&store.pool)
            .await
            .unwrap();
        store.migrate().await.unwrap();
        assert_eq!(
            store.get_log("l").await.unwrap().system_tags_json,
            "[\"custom\"]"
        );

        store.pool.close().await;
        let _ = std::fs::remove_file(path);
    }
}
