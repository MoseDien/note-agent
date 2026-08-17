# 代码清理候选

本文件记录仍保留的兼容代码；Telegram Gateway 已不再依赖这些模块。它们可在后续版本删除。

## 仍保留的兼容代码

- `src/local_llm.rs` 和 Qwen storage-decision prompt。
- `StorageAction`、`StorageDecision`、`IngestResult`。
- `ReversalStore` 和 `x` 推翻逻辑。
- Qwen 相关 `.env` 配置和 Ollama 启动说明。

## 继续保留的辅助代码

- `src/glm.rs` 和 connections 提示词：未来手动 `/analyze` 可能复用。
- `src/privacy.rs`：未来发送 Remote LLM 前仍需要。
- 旧 FTS、分类和标签兼容字段：确认数据迁移方案后再处理。

## 需要新增的代码方向

- Telegram 多媒体消息解析。
- 媒体下载和本地文件存储。
- 统一 `entries` 记录模型。
- Telegram message ID 去重。
- 媒体删除和孤立文件清理。
- `helo` 健康检查。

删除前必须检查引用、SQLite 兼容、用户隔离，并运行完整测试和 coverage。
