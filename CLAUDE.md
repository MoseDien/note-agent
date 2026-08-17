# CLAUDE.md

> 迁移说明：当前文件描述旧版 Qwen/GLM Rust MVP。下一阶段的产品边界、需求和清理计划以 `docs/` 为准；先完成文档确认，再修改业务代码。

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

`AGENTS.md` is the authoritative development guide (commands, conventions, invariants). `SOUL.md` defines the product character and behavioral boundaries. `README.md` is the user/install guide. Read those; this file captures the architecture and the invariants that are easy to break.

## What this project is

Daily Agent (`daily-agent`) — a Rust personal-journal agent with two input channels that share one SQLite store:
- **Terminal**: interactive REPL and one-shot CLI (`src/main.rs`)
- **Telegram**: long-polling gateway (`src/telegram.rs`)

Ordinary input is routed only to a **local** Ollama model (`qwen3:1.7b`) which decides `store` / `ignore` / `ask` and nothing else. **GLM** is invoked only by explicit advanced commands (`connections`). See `README.md` for the rationale behind the embedding→Ollama switch (storage intent is classification, not topic similarity).

## Commands

```bash
cargo fmt --all -- --check
cargo check
cargo clippy --all-targets -- -D warnings
cargo test
cargo llvm-cov --summary-only --fail-under-lines 90   # 90% line-coverage gate
```

Run a single test by name: `cargo test isolates_users` (substring match).

Run the app (after `cp .env.example .env` and filling secrets; `set -a; source .env; set +a`):

```bash
cargo build --release
target/release/daily-agent                                  # terminal interactive REPL
target/release/daily-agent gateway                          # Telegram gateway
target/release/daily-agent decide "some text"               # local store/ignore/ask, no save
target/release/daily-agent add "force-save, no model" --privacy no_upload
target/release/daily-agent link-telegram                    # one-time pairing code
```

Tests use a local mock HTTP server (`src/test_support.rs`), temp SQLite DBs, and never touch real Ollama / GLM / Telegram or `.env`. Keep it that way.

## Architecture

### Two adapters, one core
`main.rs` (terminal) and `telegram.rs` (telegram) are **thin adapters**. Both must call the same functions in `agent.rs` (`ingest_log`, `add_log`, `connections`, `reverse_last_decision`, `delete_log_reference`). **Never duplicate storage-decision or memory logic inside an adapter.** This is the central architectural rule.

### Module map (responsibilities, not a file listing)
- `agent.rs` — shared workflow: ingest/save, connections, deletion-by-reference, the reversal store.
- `local_llm.rs` — Ollama transport + structured-output parsing for the storage decision.
- `glm.rs` — GLM HTTP client and response validation.
- `db.rs` — SQLite persistence, idempotent migrations, FTS5 retrieval, Telegram identity pairing.
- `privacy.rs` — local PII redaction (email/phone/id-card/bank-card/ipv4) before anything leaves the machine.
- `i18n.rs` / `prompts.rs` — load runtime UI strings and LLM system prompts from `resources/`.
- `config.rs` — all settings come from environment variables.
- `models.rs` — shared domain and API types.
- `commands.rs` — shortest-unique-prefix resolution for slash commands. Both channels expand short prefixes (`/d`→`/delete`) before dispatch; `/link` and `/log` are full-word only.

### The `x` reversal
A single ordinary input is reversible for 10 minutes via `x` / `/x`. The decision is held **only in process memory** (`agent::ReversalStore`, keyed by `(internal_user_id, channel)`) — never persisted to SQLite, lost on restart. It is single-use and isolated per user+channel. Explicit `/log`, `/private`, `delete`, and advanced commands are **not** reversible.

### Identity
Telegram usernames are **not** stable identities. Linking uses one-time, single-use pairing codes that bind a Telegram numeric user ID to an internal user via the `channel_identities` table (`db.rs`). Every query and mutation must be scoped by the internal user ID.

## Critical invariants (do not break)

- **Local model is the only ordinary-input path.** It may only return `store` / `ignore` / `ask` (enforced by JSON Schema + Rust `deny_unknown_fields` + re-validation). On local-model failure, return `ask` (or honor an explicit `/log`); **never fall back to GLM** for availability.
- **The local model does not classify, tag, summarize, extract entities, infer sentiment, or score importance.**
- **GLM is gated.** Only explicit advanced commands (or future user-configured scheduled tasks) may call it. Plain input, `recent`, `delete`, `export` must never call GLM.
- **`no_upload` logs never reach GLM** — `connections` filters them out before building the candidate set.
- **Send a small, redacted candidate set to GLM — never the whole history.** Before sending, `privacy::redact`. After receiving, **verify every `source_log_id` belongs to the current user's supplied candidate set** (`agent::connections` retains only valid IDs and valid confidence ranges).
- **Don't trust model output.** Re-validate storage actions, connection kinds (allowlist in `glm.rs`), IDs, and confidence.
- **SQLite is plaintext.** Never describe storage as encrypted or end-to-end private. Never log original journal text, API keys, Telegram tokens, or redaction mappings.

## Internationalization & prompts

Exactly two locales: `zh-CN`, `en-US`. Selected only via `DAILY_AGENT_LOCALE` (no runtime language switching).

- User-facing strings → `resources/locales/{locale}.json` (`I18n::text` / `format`).
- GLM system prompts → `resources/prompts/{locale}/*.system.md` (`PromptStore`).
- **Do not put LLM prompts or new Chinese/English user-facing strings in Rust source.** Add matching keys/files for **both** locales; missing a required prompt file is a hard startup error.
- Persist language-neutral codes (`work`, `positive`, `shared_topic`, …). Translate only at display time. Don't store localized enum labels in DB rows.

## Database changes

Migrations run from `Store::migrate` on **every startup**, so new statements must be idempotent (`CREATE ... IF NOT EXISTS` pattern). Foreign keys stay on; user isolation must hold.

Legacy classification/tag columns (`category`, `analysis_status`, FTS `summary`/`topics`) may remain in existing SQLite files but are **ignored at runtime** and not written by new code. Removing them requires a separately approved destructive migration.

## Testing expectations

For behavior changes, add focused tests at the relevant boundary. Always preserve coverage for: user-data isolation, one-time pairing codes, PII redaction, both locale resource sets, external prompt availability, and invalid-locale rejection. Before handing off: `fmt`, `clippy -D warnings`, `test`, and the 90% coverage gate must all pass.
