use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};
use teloxide::{net::Download, prelude::*};

#[derive(Debug, Clone)]
pub struct MediaAttachment {
    pub content_type: String,
    pub path: PathBuf,
    pub mime_type: Option<String>,
    pub file_size: i64,
}

pub async fn download_message_media(
    bot: &Bot,
    message: &Message,
    root: &Path,
    key: &str,
    timestamp: DateTime<Utc>,
) -> Result<Option<MediaAttachment>> {
    let (file_id, content_type, extension, mime_type): (
        teloxide::types::FileId,
        &str,
        &str,
        Option<String>,
    ) = if let Some(photo) = message.photo() {
        let item = photo
            .last()
            .ok_or_else(|| anyhow::anyhow!("photo has no sizes"))?;
        (
            item.file.id.clone(),
            "photo",
            "jpg",
            Some("image/jpeg".to_owned()),
        )
    } else if let Some(voice) = message.voice() {
        (
            voice.file.id.clone(),
            "voice",
            "ogg",
            Some("audio/ogg".to_owned()),
        )
    } else if let Some(video) = message.video() {
        (
            video.file.id.clone(),
            "video",
            "mp4",
            video.mime_type.as_ref().map(ToString::to_string),
        )
    } else {
        return Ok(None);
    };
    let remote = bot.get_file(file_id).await?;
    let directory = root
        .join(timestamp.format("%Y").to_string())
        .join(timestamp.format("%m").to_string());
    tokio::fs::create_dir_all(&directory).await?;
    let path = directory.join(format!("{key}.{extension}"));
    let mut output = tokio::fs::File::create(&path).await?;
    bot.download_file(&remote.path, &mut output).await?;
    let file_size = tokio::fs::metadata(&path).await?.len() as i64;
    Ok(Some(MediaAttachment {
        content_type: content_type.into(),
        path,
        mime_type,
        file_size,
    }))
}
