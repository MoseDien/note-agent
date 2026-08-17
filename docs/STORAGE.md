# 本地存储

目标记录模型：

```sql
entries (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    telegram_chat_id INTEGER NOT NULL,
    telegram_message_id INTEGER NOT NULL,
    content_type TEXT NOT NULL,
    text TEXT,
    media_path TEXT,
    mime_type TEXT,
    file_size INTEGER,
    timestamp TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(user_id, telegram_message_id)
)
```

`timestamp` 是用户消息时间，`created_at` 是本地写入时间；第一阶段对外只强调 timestamp。

媒体目录：

```text
data/media/YYYY/MM/<entry-id>.<extension>
```

SQLite 保存元数据和路径，音频、图片、视频不直接塞进 SQLite BLOB。数据库内部使用完整随机 ID，用户回复只显示前 8 位短 ID。删除记录时同步删除媒体文件。
