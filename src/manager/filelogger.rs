use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Local;
use regex::Regex;
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;

const MAX_LOG_FILES: usize = 5;
const DEFAULT_LOG_DIR: &str = "/var/log/";

pub struct RotatingFileLogger {
    log_file_path: PathBuf,
    size_threshold: u64,
    current_size: u64,
    file: File,
    rotated_file_pattern: Regex,
    max_rotated_files: usize,
}

impl RotatingFileLogger {
    pub async fn new<P>(log_file_name: P, size_threshold: u64) -> Result<Self>
    where
        P: Into<PathBuf>,
    {
        let file_path: PathBuf = log_file_name.into();

        // Check if file
        if !file_path.is_file() {
            return Err(anyhow!("The provided path does not point to a valid file"));
        }

        // Check if correct .txt extension
        if let Some(ext) = file_path.extension() {
            if ext == "txt" {
                return Err(anyhow!("Log file must have .txt extension"));
            }
        }

        // Log file should not be directory
        if file_path.parent().is_some() {
            return Err(anyhow!(
                "The path should not contain any directories, only a filename"
            ));
        }

        let mut log_file_path = PathBuf::new();
        log_file_path.push(DEFAULT_LOG_DIR);
        log_file_path.push(file_path.clone());

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
            info!("pattern: {:?}", pattern);
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

        info!("ROTATE FUNCTION, BASE stem: {:#?}", base_stem);

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

        self.current_size = 0;

        // Enforce the maximum number of rotated files
        self.enforce_max_rotated_files().await?;

        Ok(())
    }

    async fn enforce_max_rotated_files(&self) -> Result<()> {
        info!("Running enforce_max_rot_files function...");
        info!("current log_file_path using: {:#?}", self.log_file_path);
        info!(
            "current log_file_path paretn: {:#?}",
            self.log_file_path.parent()
        );

        let dir = self
            .log_file_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Failed to get log file directory"))?;

        info!("using dir: {:#?}", dir);

        let mut entries = tokio::fs::read_dir(dir).await?;
        let mut rotated_files = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            info!("Entries in dir: {:#?}", entries);
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
        info!("rotated files after sroting {:#?}", rotated_files);

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
