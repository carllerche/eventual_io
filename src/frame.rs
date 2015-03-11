use core::*;

pub trait Frame {
    fn frame<F: Framer>(self, framer: F) -> Stream<Bytes>;

    fn frame_one<F: Framer>(self, framer: F) -> Stream<Bytes>;
}

impl Frame for Stream<Bytes> {
    fn frame<F: Framer>(self, framer: F) -> Stream<Bytes> {
        let (tx, rx) = Stream::pair();

        tx.receive(move |res| {
            if let Ok(tx) = res {
                frame(self, tx, framer);
            }
        });

        rx
    }

    fn frame_one<F: Framer>(self, framer: F) -> Stream<Bytes> {
        let (tx, rx) = Future::pair();

        tx.receive(move |res| {
            if let Ok(tx) = res {
                frame_one(self, tx, framer);
            }
        });

        rx.to_stream()
    }
}

fn frame<F>(src: Stream<Bytes>,
            dst: Sender<Bytes>,
            mut framer: F)
        where F: Framer {

    match framer.next() {
        Some(bytes) => {
            dst.send(bytes).receive(move |res| {
                if let Ok(dst) = res {
                    frame(src, dst, framer);
                }
            });
        }
        None => {
            src.receive(move |res| {
                match res {
                    Ok(Some((chunk, rest))) => {
                        framer.buffer(chunk);
                        frame(rest, dst, framer);
                    }
                    Ok(None) => {
                        if let Some(bytes) = framer.flush() {
                            dst.send(bytes);
                        }
                    }
                    Err(AsyncError::Failed(e)) => {
                        dst.fail(e);
                    }
                    Err(AsyncError::Aborted) => {
                    }
                }
            });
        }
    }
}

fn frame_one<F>(src: Stream<Bytes>,
                dst: Complete<Option<(Bytes, Stream<Bytes>)>>,
                mut framer: F)
        where F: Framer {

    src.receive(move |res| {
        match res {
            Ok(Some((chunk, rest))) => {
                framer.buffer(chunk);

                match framer.next() {
                    Some(bytes) => {
                        // Figure out if there is any data still buffered
                        let rest = match framer.flush() {
                            Some(bytes) => Future::of(Some((bytes, rest))).to_stream(),
                            None => rest,
                        };

                        dst.complete(Some((bytes, rest)));
                    }
                    None => frame_one(rest, dst, framer),
                }
            }
            Ok(None) => {
                match framer.next() {
                    Some(bytes) => dst.complete(Some((bytes, Stream::empty()))),
                    None => dst.complete(None),
                }
            }
            Err(AsyncError::Failed(e)) => {
                dst.fail(e);
            }
            Err(AsyncError::Aborted) => {}
        }
    });
}

pub trait Framer : Send {
    /// Buffer more data into the framer
    fn buffer(&mut self, bytes: Bytes);

    /// Get the next frame
    fn next(&mut self) -> Option<Bytes>;

    /// Get all buffered up data
    fn flush(&mut self) -> Option<Bytes>;
}

pub struct Len {
    len: usize,
    buf: Option<Bytes>,
}

impl Len {
    pub fn new(len: usize) -> Len {
        Len {
            len: len,
            buf: None,
        }
    }
}

impl Framer for Len {
    fn buffer(&mut self, bytes: Bytes) {
        self.buf = match self.buf.take() {
            Some(curr) => Some(curr.concat(&bytes)),
            None => Some(bytes),
        }
    }

    fn next(&mut self) -> Option<Bytes> {
        if let Some(bytes) = self.buf.take() {
            if bytes.len() > self.len {
                let (a, b) = bytes.split_at(self.len);
                self.buf = Some(b);
                return Some(a);
            } else if bytes.len() == self.len {
                return Some(bytes);
            } else {
                self.buf = Some(bytes);
            }
        }

        None
    }

    fn flush(&mut self) -> Option<Bytes> {
        self.buf.take()
    }
}
