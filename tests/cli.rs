use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

fn database() -> PathBuf {
    std::env::temp_dir().join(format!(
        "daily-agent-cli-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn run(db: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_daily-agent"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("DAILY_AGENT_DB", db)
        .env("DAILY_AGENT_LOCALE", "en-US")
        .env("DAILY_AGENT_RESOURCES", "./resources")
        .env("DAILY_AGENT_STORAGE_ENABLED", "false")
        .env_remove("GLM_API_KEY")
        .output()
        .unwrap()
}

#[test]
fn exercises_cli_commands() {
    let db = database();
    let added = run(&db, &["add", "private CLI log", "--privacy", "no-upload"]);
    assert!(added.status.success());
    let added_text = String::from_utf8(added.stdout).unwrap();
    let value: serde_json::Value = serde_json::from_str(&added_text).unwrap();
    let id = value["log"]["id"].as_str().unwrap();

    let recent = run(&db, &["recent", "--limit", "5"]);
    assert!(
        String::from_utf8(recent.stdout)
            .unwrap()
            .contains("private CLI log")
    );

    let connections = run(&db, &["connections", "--limit", "5"]);
    assert!(
        String::from_utf8(connections.stdout)
            .unwrap()
            .contains("At least two logs")
    );
    let decision = run(&db, &["decide", "Should this be stored?"]);
    assert!(
        String::from_utf8(decision.stdout)
            .unwrap()
            .contains("store_score")
    );

    let export_path = db.with_extension("json");
    let exported = run(&db, &["export", "--output", export_path.to_str().unwrap()]);
    assert!(exported.status.success());
    assert!(export_path.exists());
    assert!(run(&db, &["export"]).status.success());
    assert!(run(&db, &["link-telegram"]).status.success());
    assert!(run(&db, &["delete", id]).status.success());
    assert!(!run(&db, &["delete", "missing"]).status.success());

    let _ = std::fs::remove_file(db);
    let _ = std::fs::remove_file(export_path);
}

#[test]
fn exercises_interactive_mode_and_configuration_error() {
    let db = database();
    let mut child = Command::new(env!("CARGO_BIN_EXE_daily-agent"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("DAILY_AGENT_DB", &db)
        .env("DAILY_AGENT_LOCALE", "en-US")
        .env("DAILY_AGENT_RESOURCES", "./resources")
        .env("DAILY_AGENT_STORAGE_ENABLED", "false")
        .env_remove("GLM_API_KEY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"interactive log\n/recent\n/connections\n/exit\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("Daily Agent")
    );

    let invalid = Command::new(env!("CARGO_BIN_EXE_daily-agent"))
        .arg("recent")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("DAILY_AGENT_DB", &db)
        .env("DAILY_AGENT_LOCALE", "fr-FR")
        .env("DAILY_AGENT_RESOURCES", "./resources")
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    let _ = std::fs::remove_file(db);
}
