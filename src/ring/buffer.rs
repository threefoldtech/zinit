use failure::Error;
use std::collections::HashMap;
use std::iter::{Chain, Cloned};
use std::slice::Iter as SliceIter;
use std::sync::{Arc, Mutex};
use tokio::prelude::*;
use tokio::sync::mpsc::{
    unbounded_channel, UnboundedReceiver as Receiver, UnboundedSender as Sender,
};

struct Ring<T> {
    inner: Vec<Arc<T>>,
    at: usize,
}

pub type Iter<'a, T> = Chain<Cloned<SliceIter<'a, T>>, Cloned<SliceIter<'a, T>>>;

impl<T> Ring<T> {
    pub fn new(cap: usize) -> Ring<T> {
        Ring {
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

    pub fn push(&mut self, o: T) -> Arc<T> {
        let o = Arc::new(o);
        let r = Arc::clone(&o);
        if self.len() < self.cap() {
            self.inner.push(o);
        } else {
            self.inner[self.at] = o;
        }

        self.at = (self.at + 1) % self.cap();
        r
    }

    pub fn iter(&self) -> Vec<Arc<T>> {
        let (a, b) = self.inner.split_at(self.at);
        let mut v = vec![];
        for ob in b.iter().cloned().chain(a.iter().cloned()) {
            v.push(ob);
        }

        v
    }
}

/// AsyncBuffer is a wrapper on top of the circular buffer
/// that allow joining the buffer and streaming all the buffer content
/// and waiting for new items to be added
pub struct AsyncRing<T> {
    ring: Ring<T>,
    map: Arc<Mutex<HashMap<u64, Sender<Arc<T>>>>>,
}

impl<T> AsyncRing<T> {
    pub fn new(cap: usize) -> AsyncRing<T> {
        AsyncRing {
            ring: Ring::new(cap),
            map: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn push(&mut self, item: T) -> impl Future<Item = (), Error = ()> {
        let item = self.ring.push(item);

        let map = Arc::clone(&self.map);

        let mut futures = vec![];
        for (k, v) in map.lock().unwrap().iter() {
            let m = Arc::clone(&self.map);
            let k = k.clone();
            let f = v
                .clone()
                .send(Arc::clone(&item))
                .map(|_| ())
                .map_err(move |_| {
                    m.lock().unwrap().remove(&k);
                });
            futures.push(f);
        }

        future::collect(futures).map(|_| ())
    }

    pub fn stream(&mut self) -> impl Future<Item = BufferStream<T>, Error = ()> {
        let (tx, rx) = unbounded_channel();
        let map = Arc::clone(&self.map);

        let id: u64 = rand::random();

        tx.send_all(stream::iter_ok(self.ring.iter()))
            .map(move |(tx, _)| {
                map.lock().unwrap().insert(id, tx);
                BufferStream { rx: rx }
            })
            .map_err(|_| ())
    }
}

pub struct BufferStream<E> {
    rx: Receiver<Arc<E>>,
}

impl<E: Clone + Default> Stream for BufferStream<E> {
    type Item = E;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<E>, Error> {
        let line = match try_ready!(self.rx.poll()) {
            None => panic!("invalid channel"),
            Some(line) => line,
        };

        Ok(Async::Ready(Some(line.as_ref().clone())))
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_push() {
//         let mut buf = Ring::<i32>::new(10);

//         for i in 0..20 {
//             buf.push(i);
//         }
//         let mut l = vec![];

//         buf.scan(None, |v| l.push(*v));
//         assert_eq!(l.len(), 10);
//         assert_eq!(l[0], 10);
//     }

//     #[test]
//     fn test_advance() {
//         let mut buf = Ring::<i32>::new(10);

//         for i in 0..2 {
//             buf.push(i);
//         }

//         let (cur, value) = buf.advance(None);
//         assert_eq!(value, Some(0));

//         let (cur, value) = buf.advance(Some(cur));
//         assert_eq!(value, Some(1));

//         let (cur, value) = buf.advance(Some(cur));
//         assert_eq!(value, None);

//         buf.push(2);
//         let (_, value) = buf.advance(Some(cur));
//         assert_eq!(value, Some(2));
//     }
// }
