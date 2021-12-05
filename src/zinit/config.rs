use crate::zinit::DEFAULT_SHUTDOWN_TIMEOUT;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_yaml as yaml;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::path::Path;
pub type Services = HashMap<String, Service>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Signal {
    pub stop: String,
}

impl Default for Signal {
    fn default() -> Self {
        Signal {
            stop: String::from("sigterm"),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Log {
    None,
    Ring,
    Stdout,
}

impl Default for Log {
    fn default() -> Self {
        Log::Ring
    }
}
fn shutdown_default_timeout_fn() -> u64 {
    DEFAULT_SHUTDOWN_TIMEOUT
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Service {
    /// command to run
    pub exec: String,
    /// test command (optional)
    #[serde(default)]
    pub test: String,
    #[serde(rename = "oneshot")]
    pub one_shot: bool,
    #[serde(default = "shutdown_default_timeout_fn")]
    pub shutdown_timeout: u64,
    pub after: Vec<String>,
    pub signal: Signal,
    pub log: Log,
    pub env: HashMap<String, String>,
    pub dir: String,
}

impl Service {
    pub fn validate(&self) -> Result<()> {
        use nix::sys::signal::Signal;
        use std::str::FromStr;
        if self.exec.is_empty() {
            bail!("missing exec directive");
        }

        Signal::from_str(&self.signal.stop.to_uppercase())?;

        Ok(())
    }
}
/// load loads a single file
pub fn load<T: AsRef<Path>>(t: T) -> Result<(String, Service)> {
    let p = t.as_ref();
    //todo: can't find a way to shorten this down.
    let name = match p.file_stem() {
        Some(name) => match name.to_str() {
            Some(name) => name,
            None => bail!("invalid file name: {}", p.to_str().unwrap()),
        },
        None => bail!("invalid file name: {}", p.to_str().unwrap()),
    };

    let file = File::open(p)?;
    let service: Service = yaml::from_reader(&file)?;
    service.validate()?;
    Ok((String::from(name), service))
}

/// walks over a directory and load all configuration files.
/// the callback is called with any error that is encountered on loading
/// a file, the callback can decide to either ignore the file, or stop
/// the directory walking
pub fn load_dir<T: AsRef<Path>>(p: T) -> Result<Services> {
    let mut services: Services = HashMap::new();

    for entry in fs::read_dir(p)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }

        let fp = entry.path();

        if !matches!(fp.extension(), Some(ext) if ext == OsStr::new("yaml")) {
            continue;
        }

        let (name, service) = match load(&fp) {
            Ok(content) => content,
            Err(err) => {
                error!("failed to load config file {:?}: {}", fp, err);
                continue;
            }
        };

        services.insert(name, service);
    }

    Ok(services)
}
