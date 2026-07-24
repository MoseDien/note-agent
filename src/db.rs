use crate::models::{Connection, Log, User};
use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};
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
            "CREATE TABLE IF NOT EXISTS logs (id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE, channel TEXT NOT NULL, text TEXT NOT NULL, privacy_level TEXT NOT NULL DEFAULT 'normal', created_at TEXT NOT NULL)",
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
            created_at: Utc::now().to_rfc3339(),
        };
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO logs(id,user_id,channel,text,privacy_level,created_at) VALUES(?,?,?,?,?,?)")
            .bind(&log.id).bind(user_id).bind(channel).bind(text).bind(&log.privacy_level).bind(&log.created_at).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO logs_fts(log_id,user_id,text,summary,topics) VALUES(?,?,?,'','')")
            .bind(&log.id)
            .bind(user_id)
            .bind(text)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(log)
    }

    #[cfg_attr(not(test), allow(dead_code))]
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
    async fn indexes_plain_logs_and_cleans_derived_data() {
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
        let loaded = store.get_log(&first.id).await.unwrap();
        assert_eq!(loaded.text, "Rust privacy project");
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

        store.pool.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn reads_core_logs_from_a_legacy_database() {
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
                id,user_id,channel,text,category,created_at
             ) VALUES('l','u','terminal','legacy','情绪','2026-01-01T00:00:00Z')",
        )
        .execute(&store.pool)
        .await
        .unwrap();

        store.migrate().await.unwrap();
        store.migrate().await.unwrap();
        let legacy = store.get_log("l").await.unwrap();
        assert_eq!(legacy.text, "legacy");
        assert_eq!(legacy.privacy_level, "normal");

        store.pool.close().await;
        let _ = std::fs::remove_file(path);
    }
}
