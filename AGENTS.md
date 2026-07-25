# Daily Agent Development Guide

## Project overview

Daily Agent is a Rust personal-journal agent with two input channels:

- Terminal CLI and interactive mode
- Telegram private-chat gateway

Both channels share the same SQLite users and memories. Ollama handles ordinary input locally; GLM is reserved for explicit advanced operations.

## Technology

- Rust 2024 edition
- Tokio async runtime
- Clap CLI
- Teloxide Telegram gateway
- SQLx with SQLite and FTS5
- Reqwest GLM HTTP client
- Serde structured model responses

## Common commands

Run these from the repository root:

```bash
cargo fmt --all -- --check
cargo check
cargo test
cargo clippy -- -D warnings
cargo llvm-cov --summary-only --fail-under-lines 90
```

Start Terminal interactive mode:

```bash
cargo run
```

Start the Telegram gateway:

```bash
cargo run -- gateway
```

## Architecture

- `src/main.rs`: CLI and Terminal entry point
- `src/telegram.rs`: Telegram adapter only; keep domain logic out of it
- `src/agent.rs`: storage-decision and connection workflow
- `src/glm.rs`: GLM API transport and response validation
- `src/db.rs`: SQLite persistence, migrations, FTS, and identity pairing
- `src/privacy.rs`: local PII redaction
- `src/local_llm.rs`: local Ollama storage decision and journal classification
- `src/i18n.rs`: runtime UI resource loading
- `src/prompts.rs`: runtime prompt loading
- `src/models.rs`: shared domain and API types
- `src/config.rs`: environment-based configuration
- `src/commands.rs`: shortest-unique-prefix resolution for slash commands, shared by both channels

Terminal and Telegram must call the same agent and storage functions. Do not duplicate storage-decision or memory logic in channel adapters.

Slash commands in both channels accept their shortest unique prefix through `commands::expand_terminal` / `commands::expand_telegram` (for example `/d` means `/delete`). `/link` and `/log` are matched by full name only and must not be abbreviated. When adding a command, register it in `TELEGRAM_COMMANDS` / `TERMINAL_COMMANDS`, or in the `*_FULL_ONLY` set if it must stay full-word only.

The `x` reversal shortcut is single-use, expires after 10 minutes, and is isolated by internal user ID and channel. A rejected input may remain only in process memory during that window; do not persist it merely to support reversal. Explicit `/log`, `/private`, delete, or advanced commands must not create a reversible decision.

## Internationalization

The MVP supports exactly two locales:

- `zh-CN`
- `en-US`

The deployment locale is selected only with `DAILY_AGENT_LOCALE` in the environment. Do not add commands that let users change language dynamically.

User-facing runtime text belongs in:

```text
resources/locales/zh-CN.json
resources/locales/en-US.json
```

GLM system prompts belong in:

```text
resources/prompts/zh-CN/
resources/prompts/en-US/
```

Do not place LLM prompts or new user-facing Chinese/English messages directly in Rust source. Add matching keys or files for both locales. Missing required prompt files must remain a startup error.

Persist language-neutral codes such as `work`, `positive`, and `shared_topic`. Translate codes only at display time. Do not store localized enum labels in new database rows.

## Privacy and data handling

The MVP intentionally stores SQLite data without encryption. Do not describe it as encrypted or end-to-end private.

- Never log original journal text, API keys, Telegram tokens, or redaction mappings.
- Redact supported PII locally before sending text to GLM.
- A log with `privacy_level = no_upload` must never be sent to GLM.
- Plain input must pass local Ollama before persistence; it must never fall back to GLM. `/log` and `/private` are explicit overrides.
- Preserve the original log when GLM or network analysis fails.
- Every user query and mutation must be scoped by the internal user ID.
- Telegram usernames are not stable identities; use the stored Telegram numeric user ID mapping.
- Deleting a log must also remove or update derived indexes and connections.

The following files contain secrets or private user data and must remain ignored by Git:

```text
.env
data/*.db
data/*.db-*
```

## Model routing

The configured local Ollama model has exactly one ordinary-input responsibility: decide `store`, `ignore`, or `ask`. It must not classify, tag, summarize, extract entities, infer sentiment, or score importance. GLM may only be invoked by explicit advanced commands or user-configured scheduled tasks. A local model outage must return `ask` or preserve an explicit `/log`; it must never trigger a remote fallback.

## GLM integration

The first version uses GLM as its only advanced provider and one configured model. Keep the endpoint and model configurable through environment variables.

Require JSON responses and validate the storage action and connection codes. Do not trust model-provided source log IDs; verify that every referenced ID belongs to the current user's supplied candidate set.

Do not send the entire journal history to GLM. Use local retrieval to select a small candidate set and send only the required redacted text or summaries.

## Database changes

Migrations currently run from `Store::migrate`. New migrations must be idempotent because they execute at every startup.

Keep foreign keys enabled and preserve user isolation. When changing localized legacy values, migrate them to stable language-neutral codes.

Legacy classification and tag columns may remain physically present in an existing SQLite file, but the runtime and new exports use only core log fields. New logs must not populate or display legacy classification data. Removing legacy columns requires a separately approved destructive migration.

## Testing expectations

For behavior changes, add focused tests covering the relevant boundary. In particular, preserve tests for:

- user data isolation
- one-time Telegram pairing codes
- PII redaction
- both supported locale resource sets
- external prompt availability
- invalid locale rejection

Before handing off changes, run formatting, tests, and Clippy with warnings denied. Coverage changes must also pass the 90% line-coverage gate. GLM and Telegram tests must use local mock services rather than real credentials or external APIs.
