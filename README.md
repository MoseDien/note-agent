# Note Agent

一个使用 Rust 构建的私人记录 Agent。当前版本是 Telegram-only capture agent：接收用户发送的文字、图片、语音和视频，保存到本地，并返回简短的记录 ID。

> 本轮只整理文档，尚未修改业务代码。目标设计与当前实现的差异见 [`docs/DECISIONS.md`](docs/DECISIONS.md)。

## 目标数据流

```text
Telegram → Gateway → 解析消息 → SQLite + 本地媒体 → 收到 [hashId]
```

第一阶段不调用 Qwen、Embedding 或 Remote LLM，不自动分类、打标签、总结、转录、识别或分析内容。Remote LLM 未来只能由用户手动命令触发，不做定时任务。

## 文档

- [产品定义](docs/PRODUCT.md)
- [需求与验收标准](docs/REQUIREMENTS.md)
- [系统架构](docs/ARCHITECTURE.md)
- [Telegram 与媒体处理](docs/TELEGRAM.md)
- [本地存储](docs/STORAGE.md)
- [隐私与安全](docs/PRIVACY.md)
- [命令说明](docs/COMMANDS.md)
- [路线图](docs/ROADMAP.md)
- [设计决策](docs/DECISIONS.md)
- [代码清理候选](docs/CLEANUP-CANDIDATES.md)

## 当前代码开发说明

Telegram Gateway 已按目标设计实现。旧版 CLI、Qwen 和 GLM 模块仍保留在源码中，但不参与 Telegram 默认捕获路径，后续可在确认兼容性后移除。

## 验证命令

```bash
cargo fmt --all -- --check
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
cargo llvm-cov --summary-only --fail-under-lines 90
```
