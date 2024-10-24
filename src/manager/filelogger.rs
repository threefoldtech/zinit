use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Local;
use regex::Regex;
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;

const MAX_LOG_FILES: usize = 5;
const DEFAULT_LOG_DIR: &str = "/var/log/zinit";

pub struct RotatingFileLogger {
    log_file_path: PathBuf,
    size_threshold: u64,
    current_size: u64,
    file: File,
    rotated_file_pattern: Regex,
    max_rotated_files: usize,
}

impl RotatingFileLogger {
    pub async fn new<P>(service_name: &str, log_file_name: P, size_threshold: u64) -> Result<Self>
    where
        P: Into<PathBuf>,
    {
        let file_path: PathBuf = log_file_name.into();

        // Check if correct .txt extension
        if let Some(ext) = file_path.extension() {
            if ext != "txt" {
                bail!("Log file must have .txt extension");
            }
        }

        // Log file will be saved under `/var/log/zinit/<service_name>/<log_file_name>`
        let mut log_file_path = PathBuf::new();
        log_file_path.push(format!("{}/{}", DEFAULT_LOG_DIR, service_name));
        log_file_path.push(file_path.clone());
        debug!(
            "Creating log file for {} at {}",
            service_name,
            log_file_path.to_string_lossy()
        );

        // Create the directory if it doesn't exist
        if let Some(parent) = log_file_path.parent() {
            tokio::fs::create_dir_all(parent).await.with_context(|| {
                format!("Failed to create directories for log file: {:?}", parent)
            })?;
        }

        // Bail if there is already a log file with the same name
        if tokio::fs::metadata(&log_file_path).await.is_ok() {
            bail!("Another log file with the same name already exists; change the log file name");
        }

        // Open or create the log file
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file_path)
            .await
            .with_context(|| format!("Failed to open log file: {:?}", log_file_path))?;

        // Get current file size
        let metadata = file.metadata().await?;
        let current_size = metadata.len();

        // Compile regex to match rotated log files
        // Example: log_20230826_123456.txt
        let rotated_file_pattern = if let Some(base_name) = log_file_path.file_name() {
            let pattern = format!(
                r"^{}_\d{{8}}_\d{{6}}\.txt$",
                regex::escape(base_name.to_str().unwrap().trim_end_matches(".txt"))
            );
            Regex::new(&pattern)?
        } else {
            return Err(anyhow!("Failed to compile regex pattern"));
        };

        Ok(RotatingFileLogger {
            log_file_path,
            size_threshold,
            current_size,
            file,
            rotated_file_pattern,
            max_rotated_files: MAX_LOG_FILES,
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
            .log_file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let rotated_file_name = format!("{}_{}.txt", base_stem, datetime_str);
        let rotated_file_path = self.log_file_path.with_file_name(rotated_file_name);

        // Rename the current file to teh rotated filename
        tokio::fs::rename(&self.log_file_path, &rotated_file_path)
            .await
            .with_context(|| format!("Failed to rename log file to {:?}", rotated_file_path))?;

        // Open a new file with the original name
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_file_path)
            .await
            .with_context(|| format!("Failed to open new log file: {:?}", self.log_file_path))?;

        // After rotating, reset current size 0
        self.current_size = 0;

        // Enforce the maximum number of rotated files
        self.enforce_max_rotated_files().await?;

        Ok(())
    }

    async fn enforce_max_rotated_files(&self) -> Result<()> {
        let dir = self
            .log_file_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Failed to get log file directory"))?;

        let mut entries = tokio::fs::read_dir(dir).await?;
        let mut rotated_files = Vec::new();

        // Find the rotated files based on the log file name
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file() {
                if let Some(filename) = path.file_name().and_then(|s| s.to_str()) {
                    if self.rotated_file_pattern.is_match(filename) {
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
            }
        }

        Ok(())
    }
}
