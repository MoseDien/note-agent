mod agent;
mod config;
mod db;
mod glm;
mod models;
mod privacy;
mod telegram;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use db::Store;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "daily-agent",
    version,
    about = "A private daily-log agent for Terminal and Telegram"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Save and analyze a daily log.
    Add {
        text: String,
        #[arg(long, value_enum, default_value_t = PrivacyArg::Normal)]
        privacy: PrivacyArg,
    },
    /// Show recent logs.
    Recent {
        #[arg(short, long, default_value_t = 10)]
        limit: u32,
    },
    /// Analyze connections among recent logs.
    Connections {
        #[arg(short, long, default_value_t = 30)]
        limit: u32,
    },
    /// Delete one log and its derived memories.
    Delete { id: String },
    /// Export all logs as JSON.
    Export {
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Create a one-time code for linking Telegram.
    LinkTelegram,
    /// Start the Telegram long-polling gateway.
    Gateway,
}

#[derive(Clone, Copy, ValueEnum)]
enum PrivacyArg {
    Normal,
    NoUpload,
}

impl PrivacyArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::NoUpload => "no_upload",
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .without_time()
        .init();

    let cli = Cli::parse();
    let config = config::Config::from_env()?;
    let store = Store::connect(&config.database_url).await?;
    store.migrate().await?;

    if matches!(cli.command.as_ref(), Some(Command::Gateway)) {
        return telegram::run(store, config).await;
    }

    let user = store.ensure_local_user(&config.local_user).await?;
    match cli.command {
        None => interactive(&store, &config, &user.id).await?,
        Some(Command::Add { text, privacy }) => {
            let result = agent::add_log(
                &store,
                &config,
                &user.id,
                &text,
                "terminal",
                privacy.as_str(),
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Some(Command::Recent { limit }) => {
            for log in store.recent_logs(&user.id, limit).await? {
                println!(
                    "[{}] {} · {}\n{}\n",
                    log.id,
                    log.created_at,
                    log.category.as_deref().unwrap_or("待分析"),
                    log.summary.as_deref().unwrap_or(&log.text)
                );
            }
        }
        Some(Command::Connections { limit }) => {
            let result = agent::connections(&store, &config, &user.id, limit).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Some(Command::Delete { id }) => {
            anyhow::ensure!(store.delete_log(&user.id, &id).await?, "没有找到该记录");
            println!("已删除 {id}");
        }
        Some(Command::Export { output }) => {
            let json = serde_json::to_string_pretty(&store.export_user(&user.id).await?)?;
            if let Some(path) = output {
                std::fs::write(&path, json)
                    .with_context(|| format!("无法写入 {}", path.display()))?;
                println!("已导出到 {}", path.display());
            } else {
                println!("{json}");
            }
        }
        Some(Command::LinkTelegram) => {
            let code = store.create_pairing_code(&user.id).await?;
            println!("在 Telegram 中发送 /link {code}\n配对码 10 分钟内有效且只能使用一次。");
        }
        Some(Command::Gateway) => unreachable!(),
    }
    Ok(())
}

async fn interactive(store: &Store, config: &config::Config, user_id: &str) -> Result<()> {
    use std::io::Write;
    println!("Daily Agent · 输入日志；/recent 查看记录；/connections 分析联系；/exit 退出");
    loop {
        print!("> ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input)? == 0 {
            break;
        }
        let input = input.trim();
        match input {
            "" => continue,
            "/exit" | "/quit" => break,
            "/recent" => {
                for log in store.recent_logs(user_id, 10).await? {
                    println!("{}\n", agent::format_log(&log));
                }
            }
            "/connections" => println!(
                "{}",
                serde_json::to_string_pretty(
                    &agent::connections(store, config, user_id, 30).await?
                )?
            ),
            text => println!(
                "{}",
                serde_json::to_string_pretty(
                    &agent::add_log(store, config, user_id, text, "terminal", "normal").await?
                )?
            ),
        }
    }
    Ok(())
}
