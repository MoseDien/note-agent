# Daily Agent MVP

一个使用 Rust 构建的私人日志 Agent。Terminal 与 Telegram 共享同一个用户、SQLite memory 和 GLM 分析能力。

## 已实现

- Terminal 交互模式与单次命令
- Telegram 私聊 long polling gateway
- 一次性配对码绑定 Terminal 与 Telegram 身份
- SQLite 明文存储、用户隔离与 FTS5 全文检索
- GLM 分类、摘要、标签、实体和情绪分析
- multilingual-e5-small 本地判断普通输入是否需要存储
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
export DAILY_AGENT_LOCALE="zh-CN"
```

界面和 GLM 输出语言在 `.env` 中固定配置，只支持 `zh-CN` 和 `en-US`：

```bash
DAILY_AGENT_LOCALE=zh-CN
DAILY_AGENT_RESOURCES=./resources
```

英文部署改为：

```bash
DAILY_AGENT_LOCALE=en-US
```

修改语言后需要重启 Terminal 或 Telegram gateway。用户不能通过命令动态切换语言。界面文案位于 `resources/locales/`，GLM system prompts 位于 `resources/prompts/`；修改这些资源不需要修改 Rust 源码。

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

普通文字会先通过本地 `multilingual-e5-small` 判断：`store` 才保存，`ignore` 不保存，低置信度 `ask` 也暂不保存。`/log 内容` 可以强制保存，`/private 内容` 可以强制仅在本地保存。

## 本地存储判断

首次启动 Terminal 交互模式、Telegram gateway 或 `decide` 命令时，会下载 `intfloat/multilingual-e5-small` 到 `.fastembed_cache/`；之后可以离线加载。模型只在本地生成 embedding。

测试一段输入但不保存：

```bash
cargo run -- decide "今天完成了新的功能"
cargo run -- decide "什么是 SQLite？"
```

示例数据位于 `resources/storage-examples.json`，当前包含 `store` 和 `ignore` 各 20 条中英文示例。可以直接追加字符串，重启程序后生效。

相关配置：

```bash
DAILY_AGENT_STORAGE_ENABLED=true
DAILY_AGENT_STORAGE_EXAMPLES=./resources/storage-examples.json
DAILY_AGENT_STORAGE_MIN_SIMILARITY=0.75
DAILY_AGENT_STORAGE_MIN_MARGIN=0.03
DAILY_AGENT_STORAGE_TOP_K=3
FASTEMBED_CACHE_DIR=./.fastembed_cache
```

阈值只是初始值，应使用真实输入校准。设置 `DAILY_AGENT_STORAGE_ENABLED=false` 会关闭本地判断并恢复为普通输入直接存储。

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
cargo clippy -- -D warnings
```

覆盖率工具与 90% 门禁：

```bash
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --locked
cargo llvm-cov --summary-only --fail-under-lines 90
```

测试使用本机回环地址上的临时 mock HTTP 服务模拟 GLM 和 Telegram，不会访问真实 Provider、读取 `.env` 或操作正式用户数据库。
