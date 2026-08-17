# Telegram Gateway 与媒体处理

## 支持的消息

| 类型 | 保存内容 |
|---|---|
| text | 原始文字 |
| photo | 最高分辨率图片文件 |
| voice | 原始 OGG/Opus 文件 |
| video | 原始视频文件 |
| photo/video caption | 写入 `text` |

第一阶段只保存原始媒体，不做 OCR、转录、视觉理解或视频摘要。记录的 `timestamp` 使用 Telegram 消息时间，以 UTC 存储；下载完成时间不参与判断。

系统命令：`helo`、`/help`、`/recent`、`/delete ID`、`/delete -N`、`/export`、`/link CODE`。命令不写入日志。

Telegram 数字用户 ID 绑定内部用户 ID；username 不是稳定身份。所有读取、写入和删除必须按内部用户 ID 限定。

保存成功只回复：

```text
收到 [7f3a91c2]
```
