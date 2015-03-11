use bytes::{ByteStr, Buf, ByteBuf};
use core::{self, Async, Bytes, Pair, Sender};
use mio::{NonBlock, TryRead, TryWrite, Token};
use mio::tcp::TcpStream;
use net::Action;
use reactor::Notify;
use std::{fmt, mem};

pub struct Stream {
    io: NonBlock<TcpStream>,
    reading: Reading,
    writing: Writing,
}

impl Stream {
    pub fn of(io: NonBlock<TcpStream>) -> (Stream, Pair<Bytes>) {
        let (read_tx, read_rx) = core::Stream::pair();
        let (write_tx, write_rx) = core::Stream::pair();

        let stream = Stream {
            io: io,
            reading: Reading::New { tx: read_tx },
            writing: Writing::New { rx: write_rx },
        };

        (stream, (write_tx, read_rx))
    }

    pub fn io(&self) -> &NonBlock<TcpStream> {
        &self.io
    }

    // Initialize the stream state, returning the action that the reactor
    // should perform on the socket
    pub fn init(&mut self, notify: &Notify, token: Token) -> Action {
        let tx = self.reading.new_to_waiting();

        match tx.poll() {
            Ok(Ok(tx)) => {
                self.read_interest(tx);
            }
            Ok(Err(_)) => {
                self.reading.close()
            }
            Err(tx) => {
                self.read_wait(tx, notify, token)
            }
        }

        let rx = self.writing.new_to_waiting();

        match rx.poll() {
            Ok(Ok(Some((bytes, rx)))) => {
                self.write_interest(bytes, rx);
            }
            Ok(Ok(None)) => {
                self.writing.close()
            }
            Ok(Err(_)) => {
                panic!("[unimplemented] gotta handle errors");
            }
            Err(rx) => {
                self.write_wait(rx, notify, token);
            }
        }

        self.action()
    }

    /*
     *
     * ===== Read =====
     *
     */

    pub fn read_interest(&mut self, tx: Sender<Bytes>) -> Action {
        self.reading.waiting_to_reading(tx);
        self.action()
    }

    pub fn read_close(&mut self) -> Action {
        debug!("Stream::read_close");
        self.reading.close();
        self.action()
    }

    pub fn read(&mut self, notify: &Notify, token: Token) -> Action {
        let tx = self.reading.reading_to_waiting();

        // TODO: Use a customizable buffer allocation strategy
        let mut buf = ByteBuf::mut_with_capacity(4096);

        match self.io.read(&mut buf) {
            Ok(Some(0)) => {
                // The read end of the socket has been closed
                self.reading.close();
            }
            Ok(Some(_)) => {
                // Send bytes to teh consumer
                let busy = tx.send(buf.flip().to_bytes());
                self.read_wait(busy, notify, token);
            }
            Ok(None) => {
                // Nothing to do, re-register, put back in the reading state.
                // The socket will be re-registered with the event loop
                self.reading.waiting_to_reading(tx);
            }
            Err(_) => {
                panic!("[unimplemented] read failed");
            }
        }

        self.action()
    }

    fn read_wait<A>(&mut self, tx: A, notify: &Notify, token: Token)
            where A: Async<Value=Sender<Bytes>> {

        debug!("Stream::read_wait");

        let notify = notify.clone();
        tx.receive(move |res| {
            debug!("Stream::read_wait; the consumer has registered interest");
            match res {
                Ok(tx) => {
                    if notify.stream_read_ready(Some(tx), token) {
                        return;
                    }
                }
                Err(_) => {
                    // Consumer is no longer interested in reading
                    if notify.stream_read_ready(None, token) {
                        return;
                    }
                }
            }

            panic!("[unimplemented] failed to notify reactor of ready listener");
        });
    }

    /*
     *
     * ===== Read =====
     *
     */

    pub fn write_interest(&mut self, bytes: Bytes, rx: core::Stream<Bytes>) -> Action {
        debug!("Stream::write_interest; received data, waiting for writability");
        self.writing.waiting_to_writing(bytes.buf(), rx);
        self.action()
    }

    pub fn write_close(&mut self) -> Action {
        debug!("Stream::write_close");
        self.writing.close();
        self.action()
    }

    pub fn write(&mut self, notify: &Notify, token: Token) -> Action {
        let (mut buf, rx) = self.writing.writing_to_waiting();

        debug!("Stream::write; token={:?}", token);

        // TODO: There should probably be an iteration cap
        while buf.has_remaining() {
            match self.io.write(&mut buf) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    self.writing.waiting_to_writing(buf, rx);
                    return self.action();
                }
                Err(_) => {
                    panic!("[unimplemented] write failed");
                }
            }
        }

        // Wait for more bytes
        self.write_wait(rx, notify, token);
        self.action()
    }

    fn write_wait(&mut self, rx: core::Stream<Bytes>, notify: &Notify, token: Token) {
        debug!("Stream::write_wait; token={:?}", token);

        let notify = notify.clone();
        rx.receive(move |res| {
            match res {
                Ok(head) => {
                    if notify.stream_write_ready(head, token) {
                        return;
                    }
                }
                Err(_) => panic!("[unimplemented] consumer failed"),
            }

            panic!("[unimplemented] failed to notify reactor of ready listener");
        });
    }

    fn action(&self) -> Action {
        // Convert state to action
        match (&self.reading, &self.writing) {
            (&Reading::Reading { .. }, &Writing::Writing { .. }) => Action::read_write(),
            (&Reading::Reading { .. }, _                       ) => Action::read(),
            (_,                        &Writing::Writing { .. }) => Action::write(),
            (&Reading::Closed,         &Writing::Closed)         => Action::remove(),
            _                                                    => Action::wait(),
        }
    }
}

unsafe impl Send for Stream { }

enum Reading {
    New { tx: Sender<Bytes> },
    Waiting,
    Reading { tx: Sender<Bytes> },
    Closed,
}

impl Reading {
    fn new_to_waiting(&mut self) -> Sender<Bytes> {
        match mem::replace(self, Reading::Waiting) {
            Reading::New { tx } => tx,
            _ => panic!("unexpected state"),
        }
    }

    fn waiting_to_reading(&mut self, tx: Sender<Bytes>) {
        match mem::replace(self, Reading::Reading { tx: tx }) {
            Reading::Waiting => {},
            _ => panic!("unexpected state"),
        }
    }

    fn reading_to_waiting(&mut self) -> Sender<Bytes> {
        match mem::replace(self, Reading::Waiting) {
            Reading::Reading { tx } => tx,
            _ => panic!("unexpected state"),
        }
    }

    fn close(&mut self) {
        mem::replace(self, Reading::Closed);
    }
}

enum Writing {
    New { rx: core::Stream<Bytes> },
    Waiting,
    Writing { buf: Box<Buf+'static>, rx: core::Stream<Bytes> },
    Closed,
}

impl Writing {
    fn new_to_waiting(&mut self) -> core::Stream<Bytes> {
        match mem::replace(self, Writing::Waiting) {
            Writing::New { rx } => rx,
            _ => panic!("unexpected state"),
        }
    }

    fn waiting_to_writing(&mut self, buf: Box<Buf+'static>, rx: core::Stream<Bytes>) {
        match mem::replace(self, Writing::Writing { buf: buf, rx: rx }) {
            Writing::Waiting => {},
            _ => panic!("unexpected state"),
        }
    }

    fn writing_to_waiting(&mut self) -> (Box<Buf+'static>, core::Stream<Bytes>) {
        match mem::replace(self, Writing::Waiting) {
            Writing::Writing { buf, rx } => (buf, rx),
            _ => panic!("unexpected state"),
        }
    }

    fn close(&mut self) {
        if let Writing::Writing { .. } = mem::replace(self, Writing::Closed) {
            panic!("[unimplemented] the current buf needs to be flushd (unless
                    the sock is being closed due to an IO error");
        }
    }
}

impl fmt::Debug for Stream {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "eventual_io::Stream {{ ... }}")
    }
}
