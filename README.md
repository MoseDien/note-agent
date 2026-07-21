# Daily Agent MVP

一个使用 Rust 构建的私人日志 Agent。Terminal 与 Telegram 共享同一个用户、SQLite memory 和 GLM 分析能力。

## 已实现

- Terminal 交互模式与单次命令
- Telegram 私聊 long polling gateway
- 一次性配对码绑定 Terminal 与 Telegram 身份
- SQLite 明文存储、用户隔离与 FTS5 全文检索
- GLM 分类、摘要、标签、实体和情绪分析
- 基于候选历史记录的联系分析与证据 ID 校验
- 手机号、邮箱、身份证号、银行卡号和 IP 的本地脱敏
- 查看、删除和 JSON 导出
- GLM 不可用时先保存原文，不丢失输入

> MVP 暂不加密数据库。任何能读取数据库文件的人都能看到原文，不应直接作为公开多用户服务部署。

## 准备

需要 Rust 1.89+。创建 `.env`（程序本身不主动加载 `.env`，请在启动前加载环境变量）：

```bash
cp .env.example .env
```

国内智谱示例：

```bash
export GLM_API_KEY="..."
export GLM_BASE_URL="https://open.bigmodel.cn/api/paas/v4"
export GLM_MODEL="glm-5.2"
export TELOXIDE_TOKEN="..."
```

构建：

```bash
cargo build --release
```

## Terminal

交互输入：

```bash
cargo run
```

单次命令：

```bash
cargo run -- add "今天重新考虑了产品的隐私设计"
cargo run -- add --privacy no-upload "这条记录绝不发送给 GLM"
cargo run -- recent
cargo run -- connections
cargo run -- delete <LOG_ID>
cargo run -- export --output export.json
```

未配置 `GLM_API_KEY` 时，`add` 仍会保存日志，并将分析状态保留为 `pending`。

## Telegram

1. 通过 BotFather 创建 Bot，将 token 写入 `TELOXIDE_TOKEN`。
2. 在 Terminal 生成配对码：

```bash
cargo run -- link-telegram
```

3. 在 Telegram 私聊 Bot：

```text
/start
/link ABCD1234
```

4. 启动 gateway：

```bash
cargo run -- gateway
```

普通文字会直接保存并分析。使用 `/private 内容` 可以仅保存而不发送给 GLM。还支持 `/log`、`/recent`、`/connections`、`/delete`、`/export`、`/privacy` 和 `/help`。

## Memory 数据流

```text
Terminal / Telegram
  → 识别内部用户
  → SQLite 保存原文
  → 本地脱敏
  → GLM 生成结构化分析
  → SQLite FTS5 筛选最多 5 条候选历史记录
  → GLM 判断联系
  → 校验证据 ID 并保存联系
```

模型只接收脱敏后的当前文字或历史摘要，不接收全部原始历史记录。日志输出不会包含消息正文或 API key。

## 验证

```bash
cargo fmt --all -- --check
cargo check
cargo test
```
