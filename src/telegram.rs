use crate::{
    agent,
    config::Config,
    db::Store,
    local_llm::LocalClassifier,
    models::{AddResult, IngestResult},
};
use anyhow::{Context, Result};
use secrecy::ExposeSecret;
use teloxide::prelude::*;

pub async fn run(store: Store, config: Config) -> Result<()> {
    let token = config
        .telegram_token
        .clone()
        .context("missing TELOXIDE_TOKEN")?;
    let bot = Bot::new(token.expose_secret());
    let local_classifier = LocalClassifier::from_config(&config).await?;
    tracing::info!("Telegram gateway started");

    teloxide::repl(bot, move |bot: Bot, msg: Message| {
        let store = store.clone();
        let config = config.clone();
        let local_classifier = local_classifier.clone();
        async move {
            if let Err(error) = handle(&bot, &msg, &store, &config, &local_classifier).await {
                tracing::warn!(chat_id=%msg.chat.id, error=%error, "Telegram request failed");
                let error_text = error.to_string();
                bot.send_message(
                    msg.chat.id,
                    config
                        .i18n
                        .format("telegram.failure", &[("error", &error_text)]),
                )
                .await?;
            }
            respond(())
        }
    })
    .await;
    Ok(())
}

async fn handle(
    bot: &Bot,
    msg: &Message,
    store: &Store,
    config: &Config,
    local_classifier: &LocalClassifier,
) -> Result<()> {
    let text = match msg.text() {
        Some(text) => text.trim(),
        None => return Ok(()),
    };
    let telegram_id = i64::try_from(msg.from.as_ref().context("missing Telegram user")?.id.0)
        .context("Telegram user id is out of range")?;

    if text == "/start" {
        bot.send_message(msg.chat.id, config.i18n.text("telegram.start"))
            .await?;
        return Ok(());
    }
    if let Some(code) = text.strip_prefix("/link ") {
        if store
            .consume_pairing_code(code.trim(), telegram_id)
            .await
            .is_err()
        {
            bot.send_message(msg.chat.id, config.i18n.text("telegram.invalid_pairing"))
                .await?;
            return Ok(());
        }
        bot.send_message(msg.chat.id, config.i18n.text("telegram.linked"))
            .await?;
        return Ok(());
    }

    let user = match store.telegram_user(telegram_id).await? {
        Some(user) => user,
        None => {
            bot.send_message(msg.chat.id, config.i18n.text("telegram.not_linked"))
                .await?;
            return Ok(());
        }
    };

    match text {
        "/help" => {
            bot.send_message(msg.chat.id, config.i18n.text("telegram.help"))
                .await?
        }
        "/recent" => {
            let logs = store.recent_logs(&user.id, 10).await?;
            let output = if logs.is_empty() {
                config.i18n.text("telegram.no_logs")
            } else {
                logs.iter()
                    .map(|log| agent::format_log(log, config))
                    .collect::<Vec<_>>()
                    .join("\n\n")
            };
            bot.send_message(msg.chat.id, truncate(&output, config))
                .await?
        }
        "/connections" => {
            let result = agent::connections(store, config, &user.id, 30).await?;
            bot.send_message(
                msg.chat.id,
                truncate(&serde_json::to_string_pretty(&result)?, config),
            )
            .await?
        }
        "/export" => {
            let output = serde_json::to_string_pretty(&store.export_user(&user.id).await?)?;
            bot.send_message(msg.chat.id, truncate(&output, config))
                .await?
        }
        "/privacy" => {
            bot.send_message(msg.chat.id, config.i18n.text("telegram.privacy"))
                .await?
        }
        _ if text.starts_with("/delete ") => {
            let id = text.trim_start_matches("/delete ").trim();
            let message = if agent::delete_log_reference(store, config, &user.id, id).await? {
                config.i18n.text("telegram.deleted")
            } else {
                config.i18n.text("telegram.not_found")
            };
            bot.send_message(msg.chat.id, message).await?
        }
        _ if text.starts_with('/')
            && !text.starts_with("/log ")
            && !text.starts_with("/private ") =>
        {
            bot.send_message(msg.chat.id, config.i18n.text("telegram.unknown_command"))
                .await?
        }
        _ => {
            let content = text
                .strip_prefix("/private ")
                .or_else(|| text.strip_prefix("/log "))
                .unwrap_or(text)
                .trim();
            let response = if text.starts_with("/private ") {
                let result =
                    agent::add_log(store, config, &user.id, content, "telegram", "no_upload")
                        .await?;
                add_response(&result, config)
            } else if text.starts_with("/log ") {
                let result =
                    agent::add_log(store, config, &user.id, content, "telegram", "normal").await?;
                add_response(&result, config)
            } else {
                match agent::ingest_log(
                    store,
                    config,
                    local_classifier,
                    &user.id,
                    content,
                    "telegram",
                )
                .await?
                {
                    IngestResult::Stored { analysis, .. } => add_response(&analysis, config),
                    IngestResult::Ignored { .. } => config.i18n.text("telegram.storage_ignored"),
                    IngestResult::Ask { .. } => config.i18n.text("telegram.storage_ask"),
                }
            };
            bot.send_message(msg.chat.id, response).await?
        }
    };
    Ok(())
}

fn add_response(result: &AddResult, config: &Config) -> String {
    let mut status = if result.log.analysis_status == "complete" {
        config.i18n.category(result.log.category.as_deref())
    } else if result.log.analysis_status == "not_requested" {
        config.i18n.text("telegram.private_saved")
    } else {
        config.i18n.text("telegram.analysis_pending")
    };
    if !result.connections.is_empty() {
        status.push_str(&format!(
            "\n\n{}\n",
            config.i18n.text("telegram.connections_found")
        ));
        status.push_str(
            &result
                .connections
                .iter()
                .map(|item| format!("• {}", item.description))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    let short_id: String = result.log.id.chars().take(4).collect();
    config
        .i18n
        .format("telegram.saved", &[("id", &short_id), ("status", &status)])
}

fn truncate(value: &str, config: &Config) -> String {
    const MAX: usize = 3900;
    if value.chars().count() <= MAX {
        return value.into();
    }
    value.chars().take(MAX).collect::<String>() + "\n" + &config.i18n.text("telegram.truncated")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{config as test_config, mock_server, telegram_response};
    use reqwest::Url;
    use uuid::Uuid;

    fn message(text: &str, user_id: u64) -> Message {
        serde_json::from_value(serde_json::json!({
            "message_id": 1,
            "date": 0,
            "chat": {"id": 123, "type": "private"},
            "from": {"id": user_id, "is_bot": false, "first_name": "Tester"},
            "text": text
        }))
        .unwrap()
    }

    async fn store() -> (Store, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("daily-agent-tg-{}.db", Uuid::new_v4()));
        let store = Store::connect(&format!("sqlite://{}", path.display()))
            .await
            .unwrap();
        store.migrate().await.unwrap();
        (store, path)
    }

    #[tokio::test]
    async fn handles_pairing_commands_private_logs_and_queries() {
        let (store, path) = store().await;
        let local = store.ensure_local_user("owner").await.unwrap();
        let responses = (0..14).map(|_| telegram_response()).collect();
        let (url, server) = mock_server(responses).await;
        let config = test_config(
            format!("sqlite://{}", path.display()),
            "http://unused".into(),
            None,
        );
        let gate = LocalClassifier::disabled();
        let bot = Bot::new("123:test").set_api_url(Url::parse(&url).unwrap());

        handle(&bot, &message("/start", 42), &store, &config, &gate)
            .await
            .unwrap();
        handle(&bot, &message("ordinary log", 42), &store, &config, &gate)
            .await
            .unwrap();
        handle(&bot, &message("/link BAD", 42), &store, &config, &gate)
            .await
            .unwrap();
        let code = store.create_pairing_code(&local.id).await.unwrap();
        handle(
            &bot,
            &message(&format!("/link {code}"), 42),
            &store,
            &config,
            &gate,
        )
        .await
        .unwrap();
        for command in ["/help", "/recent", "/privacy", "/unknown"] {
            handle(&bot, &message(command, 42), &store, &config, &gate)
                .await
                .unwrap();
        }
        handle(
            &bot,
            &message("/private private Telegram log", 42),
            &store,
            &config,
            &gate,
        )
        .await
        .unwrap();
        for command in ["/recent", "/export", "/connections", "/delete missing"] {
            handle(&bot, &message(command, 42), &store, &config, &gate)
                .await
                .unwrap();
        }
        let id = store.recent_logs(&local.id, 1).await.unwrap()[0].id.clone();
        handle(
            &bot,
            &message(&format!("/delete {id}"), 42),
            &store,
            &config,
            &gate,
        )
        .await
        .unwrap();

        assert_eq!(server.await.unwrap().len(), 14);
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn truncates_only_long_messages() {
        let config = test_config("sqlite::memory:".into(), "http://unused".into(), None);
        assert_eq!(truncate("short", &config), "short");
        let long = "x".repeat(4_001);
        let result = truncate(&long, &config);
        assert!(result.contains("truncated"));
        assert!(result.len() < long.len());
    }
}
