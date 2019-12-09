mod buffer;

use buffer::AsyncRing;
use std::sync::{Arc, RwLock};
use tokio::codec::{FramedRead, FramedWrite, LinesCodec};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::prelude::*;

// TODO: may be we need to change the buffer to a buffer of u8 instead of string
// this way we can limit the amount of memory used by the daemon in bytes instead
// of number of lines. this will also really improve the allocations/de-allocations
// needs to be done by the daemon.
pub struct RingLog {
    buffer: Arc<RwLock<AsyncRing<String>>>,
}

impl RingLog {
    pub fn new(size: usize) -> RingLog {
        RingLog {
            buffer: Arc::new(RwLock::new(AsyncRing::new(size))),
        }
    }

    pub fn sink<T>(&self, out: T) -> impl Future<Item = (), Error = ()>
    where
        T: AsyncWrite,
    {
        // read lock the buffer, and send it to output
        let mut buffer = self.buffer.write().unwrap();
        let stream = buffer.stream();
        let framed = FramedWrite::new(out, LinesCodec::new());

        stream
            .and_then(|stream| {
                stream
                    .fold(framed, |framed, line| framed.send(line))
                    .map_err(|_| ())
            })
            .map_err(|_| ())
            .map(|_| ())
    }

    pub fn named_pipe<T>(&self, name: String, inner: T) -> impl Future<Item = (), Error = ()>
    where
        T: AsyncRead,
    {
        let framed = FramedRead::new(inner, LinesCodec::new());
        let buf = Arc::clone(&self.buffer);

        framed
            .map_err(|e| error!("failed to read input from client: {}", e))
            .fold(name, move |name, line| {
                buf.write()
                    .unwrap()
                    .push(format!("{}: {}", name, line))
                    .map(|_| name)
            })
            .map(|name| debug!("client '{}' disconnected", name))
    }

    /// pipe input to the ring. In this pipe mode, the name is read from the stream itself
    /// as the first line, so log writers should define them selves in the first line (name)
    /// followed with the log output.
    pub fn pipe<T>(&self, inner: T) -> impl Future<Item = (), Error = ()>
    where
        T: AsyncRead,
    {
        let framed = FramedRead::new(inner, LinesCodec::new());
        let buf = Arc::clone(&self.buffer);

        framed
            .map_err(|e| eprintln!("failed to read input from client: {}", e))
            .fold(None, move |name, line| {
                match name {
                    // first line (name is unknown) we just set the name
                    None => future::Either::A(future::ok(Some(line))),
                    // second iteration, we write the line to buffer
                    // and then yield the name again for the line after.
                    Some(name) => {
                        let f = buf
                            .write()
                            .unwrap()
                            .push(format!("{}: {}", name, line))
                            .map(|_| Some(name));

                        future::Either::B(f)
                        //future::ok(Some(name))
                    }
                }
            })
            .map(|name| {
                debug!(
                    "client '{}' disconnected",
                    name.unwrap_or_else(|| String::from("UNKNOWN"))
                )
            })
    }
}
