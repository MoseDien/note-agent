mod agent;
mod config;
mod db;
mod glm;
mod i18n;
mod local_llm;
mod models;
mod privacy;
mod prompts;
mod telegram;
#[cfg(test)]
mod test_support;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use db::Store;
use local_llm::LocalClassifier;
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
    /// Decide locally whether an input should be stored, without saving it.
    Decide { text: String },
    /// Delete one log and its derived memories.
    Delete {
        #[arg(allow_hyphen_values = true)]
        id: String,
    },
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
        None => {
            let local_classifier = LocalClassifier::from_config(&config).await?;
            interactive(&store, &config, &local_classifier, &user.id).await?
        }
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
                    agent::display_tag(&log, &config),
                    log.summary.as_deref().unwrap_or(&log.text)
                );
            }
        }
        Some(Command::Connections { limit }) => {
            let result = agent::connections(&store, &config, &user.id, limit).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Some(Command::Decide { text }) => {
            let local_classifier = LocalClassifier::from_config(&config).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&local_classifier.decide(&text).await?)?
            );
        }
        Some(Command::Delete { id }) => {
            anyhow::ensure!(
                agent::delete_log_reference(&store, &config, &user.id, &id).await?,
                config.i18n.text("terminal.log_not_found")
            );
            println!("{}", config.i18n.format("terminal.deleted", &[("id", &id)]));
        }
        Some(Command::Export { output }) => {
            let json = serde_json::to_string_pretty(&store.export_user(&user.id).await?)?;
            if let Some(path) = output {
                std::fs::write(&path, json).with_context(|| {
                    config.i18n.format(
                        "terminal.write_failed",
                        &[("path", &path.display().to_string())],
                    )
                })?;
                println!(
                    "{}",
                    config.i18n.format(
                        "terminal.exported",
                        &[("path", &path.display().to_string())]
                    )
                );
            } else {
                println!("{json}");
            }
        }
        Some(Command::LinkTelegram) => {
            let code = store.create_pairing_code(&user.id).await?;
            println!(
                "{}",
                config.i18n.format("terminal.link", &[("code", &code)])
            );
        }
        Some(Command::Gateway) => unreachable!(),
    }
    Ok(())
}

async fn interactive(
    store: &Store,
    config: &config::Config,
    local_classifier: &LocalClassifier,
    user_id: &str,
) -> Result<()> {
    use std::io::Write;
    println!("{}", config.i18n.text("interactive.welcome"));
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
                    println!("{}\n", agent::format_log(&log, config));
                }
            }
            "/connections" => println!(
                "{}",
                serde_json::to_string_pretty(
                    &agent::connections(store, config, user_id, 30).await?
                )?
            ),
            text if text.starts_with("/delete ") || text.starts_with("delete ") => {
                let reference = text
                    .strip_prefix("/delete ")
                    .or_else(|| text.strip_prefix("delete "))
                    .unwrap()
                    .trim();
                if agent::delete_log_reference(store, config, user_id, reference).await? {
                    println!(
                        "{}",
                        config.i18n.format("terminal.deleted", &[("id", reference)])
                    );
                } else {
                    println!("{}", config.i18n.text("terminal.log_not_found"));
                }
            }
            text => println!(
                "{}",
                serde_json::to_string_pretty(
                    &agent::ingest_log(store, config, local_classifier, user_id, text, "terminal")
                        .await?
                )?
            ),
        }
    }
    Ok(())
}
