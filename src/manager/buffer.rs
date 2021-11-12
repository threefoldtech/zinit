use anyhow::Result;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{mpsc, Mutex};

struct Buffer<T> {
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
}

impl<'a, T: 'a> IntoIterator for &'a Buffer<T> {
    type IntoIter = BufferIter<'a, T>;
    type Item = &'a T;

    fn into_iter(self) -> Self::IntoIter {
        let (second, first) = self.inner.split_at(self.at);

        BufferIter {
            first,
            second,
            index: 0,
        }
    }
}

pub struct BufferIter<'a, T> {
    first: &'a [T],
    second: &'a [T],
    index: usize,
}

impl<'a, T> Iterator for BufferIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        let index = self.index;
        self.index += 1;
        if index < self.first.len() {
            Some(&self.first[index])
        } else if index - self.first.len() < self.second.len() {
            Some(&self.second[index - self.first.len()])
        } else {
            None
        }
    }
}

pub type Logs = mpsc::Receiver<Arc<String>>;

#[derive(Clone)]
pub struct Ring {
    buffer: Arc<Mutex<Buffer<Arc<String>>>>,
    sender: broadcast::Sender<Arc<String>>,
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
        let line = Arc::new(line.clone());
        self.buffer.lock().await.push(Arc::clone(&line));
        self.sender.send(line)?;
        Ok(())
    }

    pub async fn stream(&self) -> Result<Logs> {
        let (tx, stream) = mpsc::channel::<Arc<String>>(100);
        let mut rx = self.sender.subscribe();
        let buffer = self
            .buffer
            .lock()
            .await
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        tokio::spawn(async move {
            for item in buffer {
                let _ = tx.send(Arc::clone(&item)).await;
            }

            loop {
                let line = match rx.recv().await {
                    Ok(line) => line,
                    Err(RecvError::Closed) => break,
                    Err(RecvError::Lagged(n)) => {
                        Arc::new(format!("[-] zinit: {} lines dropped", n))
                    }
                };

                if let Err(_) = tx.send(line).await {
                    // client disconnected.
                    break;
                }
            }
        });

        Ok(stream)
    }
}
