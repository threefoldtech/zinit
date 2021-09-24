use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_yaml as yaml;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::path::Path;

pub type Services = HashMap<String, Service>;

#[serde(default)]
#[derive(Clone, Debug, Serialize, Deserialize)]
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

#[serde(rename_all = "lowercase")]
#[derive(Clone, Debug, Deserialize)]
pub enum Log {
    Ring,
    Stdout,
}

impl Default for Log {
    fn default() -> Self {
        Log::Ring
    }
}

#[serde(default)]
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Service {
    /// command to run
    pub exec: String,
    /// test command (optional)
    #[serde(default)]
    pub test: String,
    #[serde(rename = "oneshot")]
    pub one_shot: bool,
    pub after: Vec<String>,
    pub signal: Signal,
    pub log: Log,
    pub env: HashMap<String, String>,
    pub dir: String,
}

impl Service {
    pub fn new<S: Into<String>>(exec: S, dir: S, one_shot: bool) -> Service {
        Service {
            exec: exec.into(),
            test: "".into(),
            one_shot: one_shot,
            after: Vec::new(),
            signal: Signal::default(),
            log: Log::Stdout,
            env: HashMap::new(),
            dir: dir.into(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        use nix::sys::signal::Signal;
        use std::str::FromStr;
        if self.exec.is_empty() {
            bail!("missing exec directive");
        }

        Signal::from_str(&self.signal.stop.to_uppercase())?;

        Ok(())
    }

    pub fn test_as_service(&self) -> Option<Service> {
        if self.test.is_empty() {
            return None;
        }

        let test = match self.clone() {
            Service { test, log, env, .. } => Service {
                exec: test,
                test: String::default(),
                one_shot: true,
                after: vec![],
                log,
                env,
                signal: Signal::default(),
                dir: "".into(),
            },
        };

        Some(test)
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

        if !match fp.extension() {
            Some(ext) if ext == OsStr::new("yaml") => true,
            _ => false,
        } {
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
