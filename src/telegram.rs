use crate::{agent, commands, config::Config, db::Store, media};
use anyhow::{Context, Result};
use secrecy::ExposeSecret;
use std::path::Path;
use teloxide::prelude::*;

pub async fn run(store: Store, config: Config) -> Result<()> {
    let token = config
        .telegram_token
        .clone()
        .context("missing TELOXIDE_TOKEN")?;
    let bot = Bot::new(token.expose_secret());
    tracing::info!("Telegram gateway started");

    teloxide::repl(bot, move |bot: Bot, msg: Message| {
        let store = store.clone();
        let config = config.clone();
        let media_root = config.media_root.clone();
        async move {
            if let Err(error) = handle(&bot, &msg, &store, &config, &media_root).await {
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
    media_root: &Path,
) -> Result<()> {
    let text = msg.text().or_else(|| msg.caption()).unwrap_or("").trim();
    if text == "helo" || text == "/helo" {
        bot.send_message(msg.chat.id, config.i18n.text("telegram.helo"))
            .await?;
        return Ok(());
    }
    let expanded = commands::expand_telegram(text);
    let text = expanded.as_str();
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
            let timestamp = msg.date;
            let attachment = media::download_message_media(
                bot,
                msg,
                media_root,
                &msg.id.0.to_string(),
                timestamp,
            )
            .await?;
            let content_type = attachment
                .as_ref()
                .map(|a| a.content_type.as_str())
                .unwrap_or("text");
            let path = attachment
                .as_ref()
                .map(|a| a.path.to_string_lossy().to_string());
            let log = store
                .insert_message(
                    &user.id,
                    "telegram",
                    Some(msg.id.0 as i64),
                    content_type,
                    text,
                    path.as_deref(),
                    attachment.as_ref().and_then(|a| a.mime_type.as_deref()),
                    attachment.as_ref().map(|a| a.file_size),
                    timestamp.to_rfc3339(),
                    "normal",
                )
                .await?;
            let short_id: String = log.id.chars().take(8).collect();
            bot.send_message(
                msg.chat.id,
                config.i18n.format("telegram.saved", &[("id", &short_id)]),
            )
            .await?
        }
    };
    Ok(())
}

fn truncate(value: &str, config: &Config) -> String {
    const MAX: usize = 3900;
    if value.chars().count() <= MAX {
        return value.into();
    }
    value.chars().take(MAX).collect::<String>() + "\n" + &config.i18n.text("telegram.truncated")
}
