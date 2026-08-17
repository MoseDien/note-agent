# 路线图

## Phase 1：Capture（当前目标）

- Telegram-only Gateway
- 保存文字、图片、语音、视频
- 一个 UTC timestamp
- hashId 确认
- `helo` 健康检查
- recent/delete/export/link
- 不调用任何模型

## Phase 2：手动分析（未来）

- 设计 `/analyze` 命令
- 用户指定记录或时间范围
- 本地脱敏
- Remote LLM 分类和整理
- 分析结果与原始记录分开保存

## Phase 3：本地模型（待决定）

- 是否恢复 Qwen
- Qwen 的唯一职责
- 是否需要语音转文字、图片识别或 Embedding

## Phase 4：自动化（暂不承诺）

- 定时任务、每日报告、自动标签、自动提醒
