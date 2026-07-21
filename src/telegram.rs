use crate::{agent, config::Config, db::Store};
use anyhow::{Context, Result};
use secrecy::ExposeSecret;
use teloxide::prelude::*;

pub async fn run(store: Store, config: Config) -> Result<()> {
    let token = config
        .telegram_token
        .clone()
        .context("缺少 TELOXIDE_TOKEN")?;
    let bot = Bot::new(token.expose_secret());
    tracing::info!("Telegram gateway started");

    teloxide::repl(bot, move |bot: Bot, msg: Message| {
        let store = store.clone();
        let config = config.clone();
        async move {
            if let Err(error) = handle(&bot, &msg, &store, &config).await {
                tracing::warn!(chat_id=%msg.chat.id, error=%error, "Telegram request failed");
                bot.send_message(msg.chat.id, format!("处理失败：{error}"))
                    .await?;
            }
            respond(())
        }
    })
    .await;
    Ok(())
}

async fn handle(bot: &Bot, msg: &Message, store: &Store, config: &Config) -> Result<()> {
    let text = match msg.text() {
        Some(text) => text.trim(),
        None => return Ok(()),
    };
    let telegram_id = i64::try_from(msg.from.as_ref().context("消息缺少用户身份")?.id.0)
        .context("Telegram user id 超出范围")?;

    if text == "/start" {
        bot.send_message(msg.chat.id, "欢迎使用 Daily Agent。消息会经过 Telegram；普通文字将作为日志保存并在本地脱敏后交给 GLM 分析。请先在 Terminal 运行 `daily-agent link-telegram`，然后发送 `/link 配对码`。").await?;
        return Ok(());
    }
    if let Some(code) = text.strip_prefix("/link ") {
        store.consume_pairing_code(code.trim(), telegram_id).await?;
        bot.send_message(msg.chat.id, "配对成功。现在可以直接发送日志了。")
            .await?;
        return Ok(());
    }

    let user = match store.telegram_user(telegram_id).await? {
        Some(user) => user,
        None => {
            bot.send_message(
                msg.chat.id,
                "尚未配对。请先在 Terminal 生成配对码，再发送 `/link 配对码`。",
            )
            .await?;
            return Ok(());
        }
    };

    match text {
        "/help" => bot.send_message(msg.chat.id, "普通文字或 `/log 内容`：保存并分析\n/private 内容：仅保存，不上传 GLM\n/recent：最近记录\n/connections：分析联系\n/delete ID：删除记录\n/export：导出 JSON\n/privacy：查看隐私说明").await?,
        "/recent" => {
            let logs = store.recent_logs(&user.id, 10).await?;
            let output = if logs.is_empty() { "还没有记录。".into() } else { logs.iter().map(agent::format_log).collect::<Vec<_>>().join("\n\n") };
            bot.send_message(msg.chat.id, truncate(&output)).await?
        }
        "/connections" => {
            let result = agent::connections(store, config, &user.id, 30).await?;
            bot.send_message(msg.chat.id, truncate(&serde_json::to_string_pretty(&result)?)).await?
        }
        "/export" => {
            let output = serde_json::to_string_pretty(&store.export_user(&user.id).await?)?;
            bot.send_message(msg.chat.id, truncate(&output)).await?
        }
        "/privacy" => bot.send_message(msg.chat.id, "当前版本：SQLite 明文保存原文；调用 GLM 前会遮盖手机号、邮箱、身份证号、银行卡号和 IP。Telegram 消息仍会经过 Telegram 服务器，请勿发送密码等高度敏感信息。").await?,
        _ if text.starts_with("/delete ") => {
            let id = text.trim_start_matches("/delete ").trim();
            let message = if store.delete_log(&user.id, id).await? { "记录已删除。" } else { "没有找到该记录。" };
            bot.send_message(msg.chat.id, message).await?
        }
        _ if text.starts_with('/') && !text.starts_with("/log ") && !text.starts_with("/private ") => bot.send_message(msg.chat.id, "未知命令。发送 /help 查看帮助。").await?,
        _ => {
            let privacy_level = if text.starts_with("/private ") { "no_upload" } else { "normal" };
            let content = text.strip_prefix("/private ").or_else(|| text.strip_prefix("/log ")).unwrap_or(text).trim();
            let result = agent::add_log(store, config, &user.id, content, "telegram", privacy_level).await?;
            let mut status = if result.log.analysis_status == "complete" { format!("{}\n{}", result.log.category.as_deref().unwrap_or("其他"), result.log.summary.as_deref().unwrap_or("")) } else if result.log.analysis_status == "not_requested" { "已按禁止上传模式保存，未调用 GLM。".into() } else { "已保存，但 AI 分析暂未完成。".into() };
            if !result.connections.is_empty() {
                status.push_str("\n\n发现联系：\n");
                status.push_str(&result.connections.iter().map(|item| format!("• {}", item.description)).collect::<Vec<_>>().join("\n"));
            }
            bot.send_message(msg.chat.id, format!("已记录 [{}]\n{}", result.log.id, status)).await?
        }
    };
    Ok(())
}

fn truncate(value: &str) -> String {
    const MAX: usize = 3900;
    if value.chars().count() <= MAX {
        return value.into();
    }
    value.chars().take(MAX).collect::<String>() + "\n…内容过长，已截断"
}
