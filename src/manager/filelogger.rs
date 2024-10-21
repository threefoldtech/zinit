use std::fs::metadata;
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Local;
use regex::Regex;
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::futures;

pub struct RotatingFileLogger {
    file_path: PathBuf,
    size_threshold: u64,
    current_size: u64,
    file: File,
    rotated_file_pattern: Regex,
    max_rotated_files: usize,
}

impl RotatingFileLogger {
    pub async fn new<P>(file_path: P, size_threshold: u64) -> Result<Self>
    where
        P: Into<PathBuf>,
    {
        let file_path = file_path.into();

        // Open or create the log file
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .await
            .with_context(|| format!("Failed to open log file: {:?}", file_path))?;

        // Get curretn file size
        let metadata = file.metadata().await?;
        let current_size = metadata.len();

        // Compile regex to match rotated log files
        // Example: log_20230826_123456.txt
        let base_name = file_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .trim_end_matches(".txt");

        let pattern = format!(r"^{}_\d{{8}}_\d{{6}}\.txt$", regex::escape(base_name));
        info!("pattern: {:?}", pattern);

        let rotated_file_pattern = Regex::new(&pattern)
            .with_context(|| format!("Failed to compile regex pattern: {}", pattern))?;

        Ok(RotatingFileLogger {
            file_path,
            size_threshold,
            current_size,
            file,
            rotated_file_pattern,
            max_rotated_files: 5,
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
        let base_stem = self
            .file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .trim_end_matches(".txt");

        let rotated_file_name = format!("{}_{}.txt", base_stem, datetime_str);
        let rotated_file_path = self.file_path.with_file_name(rotated_file_name);

        // Rename the current file to teh rotated filename
        tokio::fs::rename(&self.file_path, &rotated_file_path)
            .await
            .with_context(|| format!("Failed to rename log file to {:?}", rotated_file_path))?;

        // Open a new file with the original name
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)
            .await
            .with_context(|| format!("Failed to open new log file: {:?}", self.file_path))?;

        self.current_size = 0;

        // Enforce the maximum number of rotated files
        self.enforce_max_rotated_files().await?;

        Ok(())
    }

    async fn enforce_max_rotated_files(&self) -> Result<()> {
        let dir = self
            .file_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Failed to get log file directory"))?;

        let mut entries = tokio::fs::read_dir(dir).await?;
        let mut rotated_files = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file() {
                if let Some(filename) = path.file_name().and_then(|s| s.to_str()) {
                    info!("Found related logfile: {:?}", filename);
                    if self.rotated_file_pattern.is_match(filename) {
                        info!("...and matched the pattern");
                        // retrieve modification time
                        if let Ok(metadata) = tokio::fs::metadata(&path).await {
                            if let Ok(modified) = metadata.modified() {
                                rotated_files.push((path, modified));
                            }
                        }
                    }
                }
            }
        }
        // Sort by creation time
        rotated_files.sort_by_key(|(_, modtime)| *modtime);

        // If there are more than max_rotated_files, delete the oldest ones
        if rotated_files.len() > self.max_rotated_files {
            let files_to_delete = &rotated_files[..rotated_files.len() - self.max_rotated_files];
            for (file, _) in files_to_delete {
                tokio::fs::remove_file(file).await.with_context(|| {
                    format!("Failed to delete old rotated log file: {:?}", file)
                })?;
                info!("removed file: {:?}", file);
            }
        }

        Ok(())
    }
}
