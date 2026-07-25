# Daily Agent MVP

一个使用 Rust 构建的私人日志 Agent。Terminal 与 Telegram 共用 SQLite memory，本机 Qwen 只负责判断日常输入是否需要保存；GLM 只用于用户明确触发的高级分析。

> SQLite 当前明文保存原文，不要把本项目直接部署成公开多用户服务。

## 模型路由

```text
普通输入
  → 本机 Ollama / qwen3:1.7b
  → store：保存原文
  → ignore：不保存
  → ask：请求用户确认

/connections 等高级命令
  → 本地检索少量候选记录
  → 本地脱敏
  → GLM 高级分析
```

普通输入不会发送给 GLM。本地 Qwen 不可用时返回 `ask`，不会自动回退到远程模型。`/private` 强制保存且不调用任何模型。

## 为什么从 embedding 改为 Ollama + Qwen

项目早期使用 `multilingual-e5-small`，将用户输入与 `store`、`ignore` 示例分别计算 embedding 相似度，再根据分数和 margin 决定是否保存。该方案体积小、速度快、完全本地运行，但不适合当前核心问题。

“是否值得保存”主要是意图分类，而不是主题相似度。例如：

```text
今天解决了 Telegram 连接问题。
Telegram 连接问题怎么解决？
```

两句话主题高度相似，embedding 会给出接近的向量；但第一句是应保存的个人经历，第二句是应忽略的知识问题。增加到 1000 条模板示例后，这种主题重叠仍然存在，而且大量合成句式可能进一步模糊分类边界。相似度阈值也需要持续人工校准，难以解释复杂、双语或中英混合输入。

因此当前改用本机 Ollama 运行 `qwen3:1.7b`：

- Qwen 能区分个人记忆与知识问题、临时请求、命令和寒暄。
- 本地调用只返回 `store`、`ignore` 或 `ask`。
- JSON Schema 限制输出字段，Rust 端再次验证。
- 中文、英文和中英混合使用同一个模型，不依赖部署语言。
- 用户普通输入只进入本机 Ollama，不发送给 GLM。
- 本地模型失败时返回 `ask`，不会为了可用性而回退到远程模型。

这不表示 embedding 没有价值。它仍然适合全文语义检索、相似日志召回、聚类和为高级分析筛选少量候选记录。当前只是不再用 embedding 单独判断用户的存储意图。旧示例数据暂时保留作对照，但不参与运行。

## 安装

需要：

- macOS 14+
- Rust 1.89+
- Ollama

使用 Homebrew 安装 Ollama：

```bash
brew install --cask ollama
open -a Ollama
```

也可以从 Ollama 官网下载安装 macOS 应用。确认服务启动后下载本地模型：

```bash
ollama pull qwen3:1.7b
ollama list
```

验证本地 API：

```bash
curl http://127.0.0.1:11434/api/tags
```

准备 Daily Agent：

```bash
cd /Users/belldien/Documents/harness-0721
cp .env.example .env
```

如果 `.env` 已存在，不要重复复制，以免覆盖 API key。填写 Telegram 和 GLM 配置，然后加载：

```bash
set -a
source .env
set +a
```

构建：

```bash
cargo build --release
```

## 配置

```env
DAILY_AGENT_DB=./data/daily-agent.db
DAILY_AGENT_USER=default
DAILY_AGENT_LOCALE=zh-CN
DAILY_AGENT_RESOURCES=./resources

DAILY_AGENT_LOCAL_LLM_URL=http://127.0.0.1:11434
DAILY_AGENT_LOCAL_LLM_MODEL=qwen3:1.7b
DAILY_AGENT_LOCAL_LLM_TIMEOUT_SECONDS=60

GLM_API_KEY=...
GLM_BASE_URL=https://open.bigmodel.cn/api/paas/v4
GLM_MODEL=glm-5.2

TELOXIDE_TOKEN=...
```

`DAILY_AGENT_LOCALE` 只控制界面和提示词语言。Qwen 支持中文、英文和混合输入。提示词位于：

```text
resources/prompts/zh-CN/storage-decision.system.md
resources/prompts/en-US/storage-decision.system.md
```

## Terminal

测试本地存储判断但不保存：

```bash
target/release/daily-agent decide "今天完成了 Telegram 接入"
target/release/daily-agent decide "What is SQLite?"
```

启动交互模式：

```bash
target/release/daily-agent
```

普通输入完成判断后，10 分钟内输入 `x` 可以推翻最近一次判断：原本保存的记录会被删除，原本未保存的输入会被直接保存。推翻只生效一次，且只影响当前 Terminal 会话。

其他命令：

```bash
target/release/daily-agent add "强制保存并在本地分析"
target/release/daily-agent add --privacy no-upload "仅保存，不调用模型"
target/release/daily-agent recent
target/release/daily-agent connections
target/release/daily-agent delete <LOG_ID>
target/release/daily-agent delete -1
target/release/daily-agent export --output export.json
```

`connections` 是暂时保留的实验性高级功能，会调用 GLM；命令设计后续再确定。普通输入、`recent`、`delete` 和 `export` 不调用 GLM。

## Telegram

生成一次性配对码：

```bash
target/release/daily-agent link-telegram
```

在 Telegram 私聊 Bot：

```text
/link ABCD1234
```

启动 gateway：

```bash
target/release/daily-agent gateway
```

Telegram 命令：

```text
普通文字       本地 Qwen 只判断是否保存
x 或 /x        推翻最近一次普通输入的判断（10 分钟内）
/log 内容      不经模型判断，强制保存
/private 内容  强制保存，不调用模型
/connections   调用 GLM 进行高级联系分析
/recent        查看最近记录
/delete ID     按完整 ID 删除记录
/delete -N     删除倒数第 N 条记录，N 为 1 至 10
/export        导出数据
```

保存成功时 Telegram 只显示完整记录 ID 的前 4 位，不重复用户输入。数据库保留完整 ID 和原文。

`x` 只推翻同一 Telegram 用户最近一次普通输入的判断，不影响 `/log`、`/private` 或其他显式命令。对于未保存的输入，原文仅在 gateway 内存中保留最多 10 分钟以支持推翻，不写入 SQLite；gateway 重启后该临时状态消失。

## 本地存储判断

Ollama 使用 JSON Schema 返回：

```json
{
  "storage_action": "store"
}
```

Qwen 不再进行分类、打标签、总结、实体提取、情绪判断或重要度评分。短输入只回答一个问题：它是否属于值得保存的用户个人记忆。

旧数据库中的分类和标签列不会被破坏性删除，但运行时和新导出只读取核心日志字段；新记录不会继续填充或展示分类数据。

## 隐私

- SQLite 明文保存被接受日志的原文。
- 普通输入只发送到 `127.0.0.1` 的 Ollama。
- 普通输入代码路径不会调用 GLM。
- `/private` 日志及其元数据永远不会发送给 GLM。
- 用户明确调用高级分析时，只选取少量候选记录，脱敏后发送；不会发送全部历史。
- 调用 GLM 前再次遮盖手机号、邮箱、身份证、银行卡和 IPv4。
- Telegram 消息仍会经过 Telegram 服务器。
- 不要输入密码、API key 等高度敏感信息。

## GLM 与定时任务

当前暂时保留的实验性高级入口是 `connections`，不会自动运行。定时任务调度器尚未实现；未来的定时周报或月报必须通过独立高级路由调用 GLM，不能复用普通输入路径。

## 验证

```bash
cargo fmt --all -- --check
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
cargo llvm-cov --summary-only --fail-under-lines 90
```

测试使用本机 mock HTTP 服务，不访问真实 Ollama、GLM、Telegram，不读取 `.env`，也不操作正式数据库。
