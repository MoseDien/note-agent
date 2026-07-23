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

## 本地 embedding 模型

Daily Agent 使用 `intfloat/multilingual-e5-small` 判断普通输入是否值得保存。模型通过 FastEmbed 和 ONNX Runtime 在本机运行，不需要安装 Python、Ollama 或单独的模型服务，也不会为了判断是否保存而把原文发送到外部 Provider。

### 安装模型

先进入项目目录并准备环境变量：

```bash
cd /Users/belldien/Documents/harness-0721
cp .env.example .env
```

如果 `.env` 已经存在，不要再次复制，避免覆盖 API key。程序不会自动读取 `.env`，每个新 Terminal 窗口需要先加载：

```bash
set -a
source .env
set +a
```

模型不需要手工下载。第一次执行 `decide` 时，FastEmbed 会自动下载并安装模型：

```bash
cargo run --release -- decide "今天完成了新的功能"
```

第一次还会编译 release 版本，因此可能需要几分钟。模型默认缓存在：

```text
.fastembed_cache/
```

当前模型缓存大约占用 481 MB。该目录已经被 Git 忽略，不应提交到仓库。后续运行会复用本地缓存。

检查模型是否已经下载：

```bash
du -sh .fastembed_cache
```

删除 `.fastembed_cache/` 会移除本地模型，下次启动时需要重新下载。

### 验证模型

`decide` 只返回判断结果，不会保存输入，也不会调用 GLM：

```bash
target/release/daily-agent decide "今天解决了 Telegram 连接问题"
target/release/daily-agent decide "什么是 SQLite 数据库？"
```

输出示例：

```json
{
  "action": "ignore",
  "store_score": 0.82,
  "ignore_score": 0.92
}
```

结果含义：

- `store`：保存到 SQLite，然后按隐私设置决定是否调用 GLM。
- `ignore`：不保存，也不调用 GLM。
- `ask`：两个类别过于接近，暂不保存，也不调用 GLM。

Terminal 的 `add`、Telegram 的 `/log` 可以强制保存；Telegram 的 `/private` 可以强制只在本地保存。

### 启动与重启

Terminal 交互模式：

```bash
set -a
source .env
set +a
target/release/daily-agent
```

Telegram gateway：

```bash
set -a
source .env
set +a
target/release/daily-agent gateway
```

修改示例数据或阈值后必须重启进程。模型和示例 embedding 会在进程启动时加载到内存。

### 示例数据集

分类示例位于 `resources/storage-examples.json`：

```json
{
  "store": ["今天完成了一个重要功能。"],
  "ignore": ["什么是 SQLite？"]
}
```

当前初始数据集共 1000 条：

- `store` 500 条，中文和英文各 250 条。
- `ignore` 500 条，中文和英文各 250 条。

可以直接追加真实输入的人工标注版本。真实、个人化的数据通常比继续增加模板生成的数据更有价值。修改 JSON 后需要重启 Agent。

重新生成初始数据集：

```bash
python3 scripts/generate_storage_examples.py
```

该命令会覆盖整个 `resources/storage-examples.json`，包括手动添加的数据。添加个人数据后不要再次运行，除非已经备份。

### 参数配置

在 `.env` 中配置：

```bash
DAILY_AGENT_STORAGE_ENABLED=true
DAILY_AGENT_STORAGE_EXAMPLES=./resources/storage-examples.json
DAILY_AGENT_STORAGE_MIN_SIMILARITY=0.75
DAILY_AGENT_STORAGE_MIN_MARGIN=0.03
DAILY_AGENT_STORAGE_TOP_K=3
FASTEMBED_CACHE_DIR=./.fastembed_cache
```

- `MIN_SIMILARITY`：两类最高分都低于该值时返回 `ask`。
- `MIN_MARGIN`：`store_score` 与 `ignore_score` 的差小于该值时返回 `ask`。
- `TOP_K`：分别取两类最相似的多少条示例计算平均分。
- `FASTEMBED_CACHE_DIR`：模型缓存目录。

这些值只是初始配置，不代表对所有个人输入都准确。建议先收集一批真实输入和人工标签，再用独立评估集调整阈值。不要仅根据一两条测试输入反复修改参数。

设置下面的值可以临时关闭本地判断，普通输入将直接保存：

```bash
DAILY_AGENT_STORAGE_ENABLED=false
```

### 常见问题

模型下载失败：检查是否能访问 Hugging Face，然后重新运行 `cargo run --release -- decide "测试"`。已经完整下载的文件会保留在缓存目录。

一直返回 `ask`：通常表示两类示例语义重叠，或者 `MIN_MARGIN` 对当前数据过高。优先清理含义模糊的示例并加入真实标注数据，再调整阈值。

修改数据没有生效：Terminal 和 gateway 只在启动时生成示例 embedding，需要完全退出并重启。

启动很慢：1000 条示例会在每次启动时重新计算 embedding。当前版本尚未持久化示例向量，后续可加入 embedding 索引缓存来缩短启动时间。

分类仍然不准确：增加合成数据不一定提高准确率。当前算法是基于相似示例的轻量 MVP，不是训练后的二分类模型。下一步应建立真实评估集和用户反馈闭环，再决定是校准算法还是训练小型分类器。

## Memory 数据流

```text
Terminal / Telegram
  → 识别内部用户
  → 本地 multilingual-e5-small 判断是否保存
  → store：SQLite 保存原文
  → ignore / ask：停止，不保存且不调用 GLM
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
