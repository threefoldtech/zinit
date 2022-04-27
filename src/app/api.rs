use crate::zinit::{config, ZInit};
use anyhow::{Context, Result};
use nix::sys::signal;
use serde::{Deserialize, Serialize};
use serde_json::{self as encoder, Value};
use std::collections::HashMap;
use std::marker::Unpin;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufStream};
use tokio::net::{UnixListener, UnixStream};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
struct Response {
    pub state: State,
    pub body: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum State {
    Ok,
    Error,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub struct Status {
    pub name: String,
    pub pid: u32,
    pub state: String,
    pub target: String,
    pub after: HashMap<String, String>,
}

pub struct Api {
    zinit: ZInit,
    socket: PathBuf,
}

impl Api {
    pub fn new<P: AsRef<Path>>(zinit: ZInit, socket: P) -> Api {
        Api {
            zinit,
            socket: socket.as_ref().to_path_buf(),
        }
    }

    pub async fn serve(&self) -> Result<()> {
        let listener = UnixListener::bind(&self.socket).context("failed to listen for socket")?;
        loop {
            if let Ok((stream, _addr)) = listener.accept().await {
                tokio::spawn(Self::handle(stream, self.zinit.clone()));
            }
        }
    }

    async fn handle(stream: UnixStream, zinit: ZInit) {
        let mut stream = BufStream::new(stream);
        let mut cmd = String::new();
        if let Err(err) = stream.read_line(&mut cmd).await {
            error!("failed to read command: {}", err);
            let _ = stream.write_all("bad request".as_ref()).await;
        };

        let response = match Self::process(cmd, &mut stream, zinit).await {
            Ok(body) => Response {
                body,
                state: State::Ok,
            },
            Err(err) => Response {
                state: State::Error,
                body: encoder::to_value(format!("{}", err)).unwrap(),
            },
        };

        let value = match encoder::to_string(&response) {
            Ok(value) => value,
            Err(err) => {
                debug!("failed to create response: {}", err);
                return;
            }
        };

        if let Err(err) = stream.write_all(value.as_bytes()).await {
            error!("failed to send response to client: {}", err);
        };

        let _ = stream.flush().await;
    }

    async fn process(
        cmd: String,
        stream: &mut BufStream<UnixStream>,
        zinit: ZInit,
    ) -> Result<Value> {
        let parts = match shlex::split(&cmd) {
            Some(parts) => parts,
            None => bail!("invalid command syntax"),
        };

        if parts.is_empty() {
            bail!("unknown command");
        }

        match parts[0].as_ref() {
            "list" => Self::list(zinit).await,
            "shutdown" => Self::shutdown(zinit).await,
            "log" => Self::log(stream, zinit).await,
            "start" if parts.len() == 2 => Self::start(&parts[1], zinit).await,
            "stop" if parts.len() == 2 => Self::stop(&parts[1], zinit).await,
            "kill" if parts.len() == 3 => Self::kill(&parts[1], &parts[2], zinit).await,
            "status" if parts.len() == 2 => Self::status(&parts[1], zinit).await,
            "forget" if parts.len() == 2 => Self::forget(&parts[1], zinit).await,
            "monitor" if parts.len() == 2 => Self::monitor(&parts[1], zinit).await,
            _ => bail!("unknown command '{}' or wrong arguments count", parts[0]),
        }
    }

    async fn log(stream: &mut BufStream<UnixStream>, zinit: ZInit) -> Result<Value> {
        let mut logs = zinit.logs().await?;

        while let Some(line) = logs.recv().await {
            stream.write_all(line.as_bytes()).await?;
            stream.write_all(b"\n").await?;
            stream.flush().await?;
        }

        Ok(Value::Null)
    }

    async fn list(zinit: ZInit) -> Result<Value> {
        let services = zinit.list().await?;
        let mut map: HashMap<String, String> = HashMap::new();
        for service in services {
            let state = zinit.status(&service).await?;
            map.insert(service, format!("{:?}", state.state));
        }

        Ok(encoder::to_value(map)?)
    }

    async fn monitor<S: AsRef<str>>(name: S, zinit: ZInit) -> Result<Value> {
        let (name, service) = config::load(format!("{}.yaml", name.as_ref()))
            .context("failed to load service config")?;
        zinit.monitor(name, service).await?;
        Ok(Value::Null)
    }

    async fn forget<S: AsRef<str>>(name: S, zinit: ZInit) -> Result<Value> {
        zinit.forget(name).await?;
        Ok(Value::Null)
    }

    async fn stop<S: AsRef<str>>(name: S, zinit: ZInit) -> Result<Value> {
        zinit.stop(name).await?;
        Ok(Value::Null)
    }

    async fn shutdown(zinit: ZInit) -> Result<Value> {
        tokio::spawn(async move {
            if let Err(err) = zinit.shutdown().await {
                error!("failed to execute shutdown: {}", err);
            }
        });

        Ok(Value::Null)
    }

    async fn start<S: AsRef<str>>(name: S, zinit: ZInit) -> Result<Value> {
        zinit.start(name).await?;
        Ok(Value::Null)
    }

    async fn kill<S: AsRef<str>>(name: S, sig: S, zinit: ZInit) -> Result<Value> {
        let sig = sig.as_ref();
        let sig = signal::Signal::from_str(&sig.to_uppercase())?;
        zinit.kill(name, sig).await?;
        Ok(Value::Null)
    }

    async fn status<S: AsRef<str>>(name: S, zinit: ZInit) -> Result<Value> {
        let status = zinit.status(&name).await?;

        let result = Status {
            name: name.as_ref().into(),
            pid: status.pid.as_raw() as u32,
            state: format!("{:?}", status.state),
            target: format!("{:?}", status.target),
            after: {
                let mut after = HashMap::new();
                for service in status.service.after {
                    let status = match zinit.status(&service).await {
                        Ok(dep) => dep.state,
                        Err(_) => crate::zinit::State::Unknown,
                    };
                    after.insert(service, format!("{:?}", status));
                }
                after
            },
        };

        Ok(encoder::to_value(result)?)
    }
}

pub struct Client {
    socket: PathBuf,
}

impl Client {
    pub fn new<P: AsRef<Path>>(socket: P) -> Client {
        Client {
            socket: socket.as_ref().to_path_buf(),
        }
    }

    async fn connect(&self) -> Result<UnixStream> {
        Ok(UnixStream::connect(&self.socket).await?)
    }

    async fn command(&self, c: &str) -> Result<Value> {
        let mut con = BufStream::new(self.connect().await?);

        con.write(c.as_bytes()).await?;
        con.write(b"\n").await?;
        con.flush().await?;

        let mut data = String::new();
        con.read_to_string(&mut data).await?;

        let response: Response = encoder::from_str(&data)?;

        match response.state {
            State::Ok => Ok(response.body),
            State::Error => {
                let err: String = encoder::from_value(response.body)?;
                bail!(err)
            }
        }
    }

    pub async fn logs<O: tokio::io::AsyncWrite + Unpin, S: AsRef<str>>(
        &self,
        mut out: O,
        filter: Option<S>,
    ) -> Result<()> {
        let mut con = self.connect().await?;
        con.write_all(b"log\n").await?;
        con.flush().await?;
        match filter {
            None => tokio::io::copy(&mut con, &mut out).await?,
            Some(filter) => {
                let filter = format!("{}:", filter.as_ref());
                let mut stream = BufStream::new(con);
                loop {
                    let mut line = String::new();
                    match stream.read_line(&mut line).await {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(err) => {
                            bail!("failed to read stream: {}", err);
                        }
                    }

                    if line[4..].starts_with(&filter) {
                        let _ = out.write_all(line.as_bytes()).await;
                    }
                }
                0
            }
        };

        Ok(())
    }

    pub async fn list(&self) -> Result<HashMap<String, String>> {
        let response = self.command("list").await?;
        Ok(encoder::from_value(response)?)
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.command("shutdown").await?;
        Ok(())
    }

    pub async fn status<S: AsRef<str>>(&self, name: S) -> Result<Status> {
        let response = self.command(&format!("status {}", name.as_ref())).await?;
        Ok(encoder::from_value(response)?)
    }

    pub async fn start<S: AsRef<str>>(&self, name: S) -> Result<()> {
        self.command(&format!("start {}", name.as_ref())).await?;
        Ok(())
    }

    pub async fn stop<S: AsRef<str>>(&self, name: S) -> Result<()> {
        self.command(&format!("stop {}", name.as_ref())).await?;
        Ok(())
    }

    pub async fn forget<S: AsRef<str>>(&self, name: S) -> Result<()> {
        self.command(&format!("forget {}", name.as_ref())).await?;
        Ok(())
    }

    pub async fn monitor<S: AsRef<str>>(&self, name: S) -> Result<()> {
        self.command(&format!("monitor {}", name.as_ref())).await?;
        Ok(())
    }

    pub async fn kill<S: AsRef<str>>(&self, name: S, sig: S) -> Result<()> {
        self.command(&format!("kill {} {}", name.as_ref(), sig.as_ref()))
            .await?;
        Ok(())
    }
}
