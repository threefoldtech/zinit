use failure::Error;
use std::sync::{Arc, RwLock, RwLockReadGuard};
use tokio::prelude::*;
use tokio::sync::watch;

pub struct Block<T> {
    data: RwLock<Option<T>>,
    n: RwLock<Option<Arc<Block<T>>>>,
}

type Ref<'a, T> = RwLockReadGuard<'a, T>;

impl<T> Block<T> {
    fn new(size: usize) -> Arc<Block<T>> {
        let head = Arc::new(Block {
            data: RwLock::new(None),
            n: RwLock::new(None),
        });

        let mut current = Arc::clone(&head);
        for _i in 1..size {
            let next = Arc::new(Block {
                data: RwLock::new(None),
                n: RwLock::new(Some(Arc::clone(&current))),
            });

            current.n.write().unwrap().replace(Arc::clone(&next));
            current = next;
        }
        current.n.write().unwrap().replace(Arc::clone(&head));
        head
    }

    fn is_empty(&self) -> bool {
        self.data.read().unwrap().is_none()
    }

    fn set(&self, d: T) {
        self.data.write().unwrap().replace(d);
    }

    fn get(&self) -> Ref<Option<T>> {
        self.data.read().unwrap()
    }

    fn next(&self) -> Arc<Block<T>> {
        Arc::clone(self.n.read().unwrap().as_ref().unwrap())
    }
}

type Cursor<T> = Arc<Block<T>>;

pub struct Ring<T> {
    current: Cursor<T>,
}

impl<T> Ring<T> {
    pub fn new(size: usize) -> Ring<T> {
        Ring {
            current: Block::new(size),
        }
    }

    pub fn push(&mut self, o: T) {
        self.current.set(o);
        self.current = self.current.next();
    }

    fn first(&self) -> Cursor<T> {
        let mut head = Arc::clone(&self.current);

        loop {
            if !head.is_empty() {
                break;
            }

            head = head.next();
            if Arc::ptr_eq(&head, &self.current) {
                break;
            }
        }

        head
    }

    pub fn scan<F>(&self, cur: Option<Cursor<T>>, mut f: F) -> Cursor<T>
    where
        F: FnMut(&T),
    {
        let mut head = match cur {
            Some(cur) => cur,
            None => Arc::clone(&self.current),
        };

        loop {
            if !head.is_empty() {
                f(head.get().as_ref().unwrap());
            }

            head = head.next();
            if Arc::ptr_eq(&head, &self.current) {
                break;
            }
        }

        head
    }
}

impl<T: Clone> Ring<T> {
    /// advances the cursor one step, returning the next value
    /// if the cursor is at the 'head' of the ring, None is returned
    fn advance(&self, head: Option<Cursor<T>>) -> (Cursor<T>, Option<T>) {
        let head = match head {
            // None means we starting a new scan, we use the head
            // of the ring.
            None => self.first(), //Arc::clone(&self.current),
            // if we continuing a scan, and we already at the head
            // it means we did a full cycle. so we stop, otherwise
            // use the given cursor
            Some(head) => {
                if Arc::ptr_eq(&head, &self.current) {
                    return (head, None);
                }

                head
            }
        };

        let r = head.get();
        let v = match r.as_ref() {
            Some(v) => Some(v.clone()),
            None => None,
        };

        drop(r);

        match v {
            Some(v) => (head.next(), Some(v)),
            None => (head, None),
        }
    }
}
/// AsyncBuffer is a wrapper on top of the circular buffer
/// that allow joining the buffer and streaming all the buffer content
/// and waiting for new items to be added
pub struct AsyncRing<E: Clone> {
    ring: Arc<RwLock<Ring<E>>>,
    rx: watch::Receiver<usize>,
    tx: watch::Sender<usize>,
    signal: usize,
}

impl<E: Clone> AsyncRing<E> {
    pub fn new(cap: usize) -> AsyncRing<E> {
        let (tx, rx) = watch::channel(0);
        AsyncRing {
            ring: Arc::new(RwLock::new(Ring::new(cap))),
            rx,
            tx,
            signal: 0,
        }
    }

    pub fn push(&mut self, item: E) -> Result<(), Error> {
        let mut buffer = self.ring.write().unwrap();

        buffer.push(item);
        self.signal = match self.signal.checked_add(1) {
            Some(v) => v,
            None => 0,
        };

        match self.tx.broadcast(self.signal) {
            Ok(_) => Ok(()),
            Err(e) => bail!("failed to notify receivers: {}", e),
        }
    }

    pub fn stream(&self) -> BufferStream<E> {
        BufferStream {
            ring: Arc::clone(&self.ring),
            cursor: None,
            rx: self.rx.clone(),
        }
    }
}

pub struct BufferStream<E> {
    ring: Arc<RwLock<Ring<E>>>,
    cursor: Option<Cursor<E>>,
    rx: watch::Receiver<usize>,
}

impl<E: Clone + Default> Stream for BufferStream<E> {
    type Item = E;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<E>, Error> {
        let ring = self.ring.read().unwrap();

        loop {
            let (cursor, value) = ring.advance(self.cursor.take());
            self.cursor = Some(cursor);
            match value {
                Some(v) => {
                    return Ok(Async::Ready(Some(v)));
                }
                None => {
                    match try_ready!(self.rx.poll()) {
                        None => panic!("invalid watch"),
                        Some(_) => {}
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push() {
        let mut buf = Ring::<i32>::new(10);

        for i in 0..20 {
            buf.push(i);
        }
        let mut l = vec![];

        buf.scan(None, |v| l.push(*v));
        assert_eq!(l.len(), 10);
        assert_eq!(l[0], 10);
    }

    #[test]
    fn test_advance() {
        let mut buf = Ring::<i32>::new(10);

        for i in 0..2 {
            buf.push(i);
        }

        let (cur, value) = buf.advance(None);
        assert_eq!(value, Some(0));

        let (cur, value) = buf.advance(Some(cur));
        assert_eq!(value, Some(1));

        let (cur, value) = buf.advance(Some(cur));
        assert_eq!(value, None);

        buf.push(2);
        let (_, value) = buf.advance(Some(cur));
        assert_eq!(value, Some(2));
    }
}
