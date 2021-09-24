use anyhow::Result;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::{mpsc, Mutex};

struct Buffer<T: Clone> {
    inner: Vec<T>,
    at: usize,
}

impl<T: Clone> Buffer<T> {
    pub fn new(cap: usize) -> Buffer<T> {
        Buffer {
            inner: Vec::with_capacity(cap),
            at: 0,
        }
    }

    fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn cap(&self) -> usize {
        self.inner.capacity()
    }

    pub fn push(&mut self, o: T) {
        if self.len() < self.cap() {
            self.inner.push(o);
        } else {
            self.inner[self.at] = o;
        }

        self.at = (self.at + 1) % self.cap();
    }

    pub fn iter(&self) -> Vec<T> {
        let (a, b) = self.inner.split_at(self.at);

        b.iter().cloned().chain(a.iter().cloned()).collect()
    }
}

pub type Logs = mpsc::Receiver<String>;

#[derive(Clone)]
pub struct Ring {
    buffer: Arc<Mutex<Buffer<String>>>,
    sender: broadcast::Sender<String>,
}

impl Ring {
    pub fn new(cap: usize) -> Ring {
        let (tx, _) = broadcast::channel(100);
        Ring {
            buffer: Arc::new(Mutex::new(Buffer::new(cap))),
            sender: tx,
        }
    }

    pub async fn push(&self, line: String) -> Result<()> {
        self.buffer.lock().await.push(line.clone());
        self.sender.send(line.clone())?;
        Ok(())
    }

    pub async fn stream(&self) -> Result<Logs> {
        let (tx, stream) = mpsc::channel::<String>(100);
        let mut rx = self.sender.subscribe();
        let buffer = self.buffer.lock().await.iter();
        tokio::spawn(async move {
            for item in buffer {
                let _ = tx.send(item).await;
            }

            while let Ok(line) = rx.recv().await {
                if let Err(_) = tx.send(line).await {
                    // log here!
                    break;
                }
            }
        });

        Ok(stream)
    }
}
