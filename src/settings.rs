use serde::{Deserialize, Serialize};
use serde_yaml as yaml;
use std::collections::HashMap;
use std::default::Default;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, DirEntry, File};
use std::path::Path;

#[derive(Debug)]
struct SettingsError(String);

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl SettingsError {
    fn new(s: &str) -> Box<SettingsError> {
        let e = SettingsError(String::from(s));
        Box::new(e)
    }
}

impl Error for SettingsError {}

type Result<T> = std::result::Result<T, Box<Error>>;
pub type Services = HashMap<String, Service>;

#[serde(default)]
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Service {
    pub exec: String,
    #[serde(rename = "oneshot")]
    pub one_shot: bool,
    pub after: Vec<String>,
}

/// load loads a single file
pub fn load<T: AsRef<Path>>(t: T) -> Result<(String, Service)> {
    let p = t.as_ref();
    //todo: can't find a way to shorten this down.
    let name = match p.file_stem() {
        Some(name) => match name.to_str() {
            Some(name) => name,
            None => return Err(SettingsError::new("invalid file name")),
        },
        None => return Err(SettingsError::new("invalid file name")),
    };

    let file = File::open(p)?;
    let service: Service = yaml::from_reader(&file)?;

    Ok((String::from(name), service))
}

pub enum Walk {
    Stop,
    Continue,
}

pub fn walk<T: AsRef<Path>, F>(p: T, cb: F) -> Result<Services>
where
    F: Fn(&Path, &Box<Error>) -> Walk,
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
