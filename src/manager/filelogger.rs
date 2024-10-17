use anyhow::{Context, Result};
use chrono::Local;
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;

pub struct RotatingFileLogger {
    file_path: String,
    size_threshold: u64,
    current_size: u64,
    file: File,
}

impl RotatingFileLogger {
    pub async fn new(file_path: impl Into<String>, size_threshold: u64) -> Result<Self> {
        let file_path = file_path.into();

        // Open or create the log file
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .await
            .with_context(|| format!("Failed to open log file: {}", file_path))?;

        // Get curretn file size
        let metadata = file.metadata().await?;
        let current_size = metadata.len();

        Ok(RotatingFileLogger {
            file_path,
            size_threshold,
            current_size,
            file,
        })
    }

    pub async fn push(&mut self, log_line: &str) -> Result<()> {
        let log_line_size = log_line.len() as u64 + 1; // +1 for newline
        if log_line_size + self.current_size > self.size_threshold {
            self.rotate().await?;
        }

        self.file.write_all(log_line.as_bytes()).await?;
        self.file.write_all(b"\n").await?;
        self.file.flush().await?;

        self.current_size += log_line_size;
        Ok(())
    }

    async fn rotate(&mut self) -> Result<()> {
        // Close current file
        self.file.sync_all().await?;

        // Generate new filename with date and time
        let now = Local::now();
        let datetime_str = now.format("%Y%m%d_%H%M%S").to_string();
        let rotated_file_name = format!(
            "{}_{}.txt",
            self.file_path.trim_end_matches(".txt"),
            datetime_str
        );

        // Rename the current file to teh rotated filename
        tokio::fs::rename(&self.file_path, &rotated_file_name)
            .await
            .with_context(|| format!("Failed to rename log file to {}", rotated_file_name))?;

        // Open a new file with the original name
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)
            .await
            .with_context(|| format!("Failed to open new log file: {}", self.file_path))?;

        self.current_size = 0;

        Ok(())
    }
}
