use failure::Error;
use serde::{Deserialize, Serialize};
use serde_yaml as yaml;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::path::Path;

#[derive(Debug, Fail)]
enum SettingsError {
    #[fail(display = "invalid file: {}", name)]
    InvalidFile { name: String },
}

type Result<T> = std::result::Result<T, Error>;
pub type Services = HashMap<String, Service>;

#[serde(default)]
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Service {
    pub exec: String,
    #[serde(rename = "oneshot")]
    pub one_shot: bool,
    pub after: Vec<String>,
}

impl Clone for Service {
    fn clone(&self) -> Self {
        return Service {
            exec: self.exec.clone(),
            one_shot: self.one_shot,
            after: self.after.clone(),
        };
    }
}

/// load loads a single file
pub fn load<T: AsRef<Path>>(t: T) -> Result<(String, Service)> {
    let p = t.as_ref();
    //todo: can't find a way to shorten this down.
    let name = match p.file_stem() {
        Some(name) => match name.to_str() {
            Some(name) => name,
            None => bail!(SettingsError::InvalidFile {
                name: String::from(p.to_str().unwrap())
            }),
        },
        None => bail!(SettingsError::InvalidFile {
            name: String::from(p.to_str().unwrap())
        }),
    };

    let file = File::open(p)?;
    let service: Service = yaml::from_reader(&file)?;

    Ok((String::from(name), service))
}

pub enum Walk {
    Stop,
    Continue,
}

/// walks over a directory and load all configuration files.
/// the callback is called with any error that is encountered on loading
/// a file, the callback can decide to either ignore the file, or stop
/// the directory walking
pub fn load_dir<T: AsRef<Path>, F>(p: T, cb: F) -> Result<Services>
where
    F: Fn(&Path, &Error) -> Walk,
{
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
            Err(err) => match cb(&fp, &err) {
                Walk::Continue => continue,
                Walk::Stop => {
                    return Err(err);
                }
            },
        };

        services.insert(name, service);
    }

    Ok(services)
}
