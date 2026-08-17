# 命令说明

| 命令 | 保存为记录 | 作用 |
|---|---:|---|
| `helo` / `/helo` | 否 | 检查 Bot、Gateway、SQLite 链路 |
| `/help` | 否 | 显示帮助 |
| `/recent` | 否 | 查看最近记录 |
| `/delete ID` | 否 | 删除指定记录及媒体 |
| `/delete -N` | 否 | 删除最近第 N 条，N 为 1–10 |
| `/export` | 否 | 导出用户数据 |
| `/link CODE` | 否 | Telegram 配对 |
| `/analyze ...` | 否，未来功能 | 手动调用 Remote LLM |

`hello`、`你好` 等普通文字不是命令，应作为用户记录保存。`helo` 成功响应示例：`OK · Gateway online`，不写入 SQLite、不调用模型。
