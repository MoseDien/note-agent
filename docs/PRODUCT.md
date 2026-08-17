# 产品定义

## 一句话定义

Note Agent 是一个 Telegram-only capture agent：接收用户内容、原样本地保存，并返回简短的记录 ID。

## 第一阶段必须做

- Telegram 作为唯一 Gateway。
- 接收文字、图片、语音、视频以及媒体 caption。
- 每条记录保存一个 Telegram 消息时间戳，以 UTC 存储。
- 保存成功后只回复 `收到 [hashId]`。
- 提供 `helo` 健康检查。
- 提供 recent、delete、export 和 Telegram 配对能力。
- 用户数据按内部用户 ID 隔离。

## 第一阶段明确不做

- 不调用本地 Qwen、Embedding 或 Remote LLM。
- 不做分类、标签、摘要、实体或情绪分析。
- 不做语音转录、图片识别或视频理解。
- 不做定时任务、自动日报、提醒或自动执行。

## 产品原则

> 只接收，可靠保存，简单确认；不理解、不扩写、不行动。

普通文字（包括 `hello`、`你好`）是用户内容；`helo`、`/recent`、`/delete` 等系统命令不保存为日志。
