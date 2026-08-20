# 命令说明

| 命令 | 保存为记录 | 作用 |
|---|---:|---|
| `/h` / `/helo` | 否 | 检查 Bot、Gateway、SQLite 链路 |
| `/help` | 否 | 显示帮助 |
| `/r` / `/recent` | 否 | 查看最近记录 |
| `/d ID` / `/delete ID` | 否 | 删除指定记录及媒体 |
| `/d -N` / `/delete -N` | 否 | 删除最近第 N 条，N 为 1–10 |
| `/e` / `/export` | 否 | 导出用户数据 |
| `/link CODE` | 否 | Telegram 配对 |
| `classify` / `/classify` | 否 | 使用 GLM 为全部未分类记录批量添加英文分类；每批最多 1000 条 |
| `/s <category>` | 否 | 显示最近 20 条指定分类记录 |
| `/a` | 否 | 显示所有可用分类 |
| `/analyze ...` | 否，未来功能 | 手动调用 Remote LLM |

`hello`、`你好` 等普通文字不是命令，应作为用户记录保存。成功保存只回复 `收到 [前4位hashId]`。`/h` 不写入 SQLite、不调用模型。

内置分类名固定使用英文：`belief`、`idea`、`plan`、`activity`、`mood`、`reminder`、`health`、`other`。分类描述可以是中文，但数据中的 category name 不翻译。
