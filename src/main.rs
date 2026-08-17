mod agent;
mod commands;
mod config;
mod db;
mod glm;
mod i18n;
mod local_llm;
mod media;
mod models;
mod privacy;
mod prompts;
mod telegram;
#[cfg(test)]
mod test_support;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use db::Store;
use local_llm::LocalStorageGate;
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
    /// Save a daily log without a model decision.
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
    // Load local development configuration when running from the project root.
    // Existing environment variables take precedence, as handled by dotenvy.
    let _ = dotenvy::dotenv();
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
            let storage_gate = LocalStorageGate::from_config(&config).await?;
            interactive(&store, &config, &storage_gate, &user.id).await?
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
                println!("{}\n", agent::format_log(&log, &config));
            }
        }
        Some(Command::Connections { limit }) => {
            let result = agent::connections(&store, &config, &user.id, limit).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Some(Command::Decide { text }) => {
            let storage_gate = LocalStorageGate::from_config(&config).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&storage_gate.decide(&text).await?)?
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
    storage_gate: &LocalStorageGate,
    user_id: &str,
) -> Result<()> {
    use std::io::Write;
    let reversals = agent::ReversalStore::default();
    println!("{}", config.i18n.text("interactive.welcome"));
    loop {
        print!("> ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input)? == 0 {
            break;
        }
        let trimmed = input.trim();
        let expanded = commands::expand_terminal(trimmed);
        let input = expanded.as_str();
        match input {
            "" => continue,
            "/exit" | "/quit" => break,
            "x" | "/x" => {
                match agent::reverse_last_decision(store, config, &reversals, user_id, "terminal")
                    .await?
                {
                    agent::ReversalOutcome::Stored(result) => {
                        let short_id: String = result.log.id.chars().take(4).collect();
                        println!(
                            "{}",
                            config.i18n.format("override.saved", &[("id", &short_id)])
                        );
                    }
                    agent::ReversalOutcome::Deleted { log_id } => {
                        let short_id: String = log_id.chars().take(4).collect();
                        println!(
                            "{}",
                            config.i18n.format("override.deleted", &[("id", &short_id)])
                        );
                    }
                    agent::ReversalOutcome::Unavailable => {
                        println!("{}", config.i18n.text("override.unavailable"));
                    }
                }
            }
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
            text => {
                let result =
                    agent::ingest_log(store, config, storage_gate, user_id, text, "terminal")
                        .await?;
                reversals.remember(user_id, "terminal", text, &result).await;
                println!("{}", serde_json::to_string_pretty(&result)?);
                println!("{}", config.i18n.text("override.hint"));
            }
        }
    }
    Ok(())
}
