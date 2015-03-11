use core::{self, async, Async, Bytes, Pair, Sender};
use mio::{NonBlock, Token};
use mio::tcp::TcpListener;
use net::{Action, Stream};
use reactor::Notify;
use std::{fmt, mem};

// ## Implementation notes
//
// It's important that a listener is never dropped while in the Waiting
// state as the reactor may receive events with the token at a future time. As
// long as this is possible, the listener must stay in the slab

pub struct Listener {
    io: NonBlock<TcpListener>,
    state: State,
}

impl Listener {
    pub fn of(io: NonBlock<TcpListener>) -> (Listener, core::Stream<Pair<Bytes>>) {
        // Core Stream
        let (tx, rx) = async::Pair::pair();
        (Listener::new(io, tx), rx)
    }

    fn new(io: NonBlock<TcpListener>, tx: Sender<Pair<Bytes>>) -> Listener {
        Listener {
            io: io,
            state: State::New { tx: tx },
        }
    }

    pub fn io(&self) -> &NonBlock<TcpListener> {
        &self.io
    }

    pub fn listen(&mut self, notify: &Notify, token: Token) -> Action {
        let tx = self.state.new_to_waiting();

        if tx.is_ready() {
            if tx.is_err() {
                return Action::remove();
            } else {
                return self.ready(tx);
            }
        }

        self.wait(tx, notify, token);
        Action::wait()
    }

    pub fn ready(&mut self, tx: Sender<Pair<Bytes>>) -> Action {
        self.state.waiting_to_listening(tx);
        Action::read()
    }

    pub fn accept(&mut self, notify: &Notify, token: Token) -> Option<(Stream, Action)> {
        // Get the sender
        let tx = self.state.listening_to_waiting();

        // Accept a socket
        let sock = match self.io.accept() {
            Ok(Some(sock)) => sock,
            Ok(None) => unimplemented!(),
            Err(_) => panic!("[unimplemented] failed to accept socket"),
        };

        // Build the stream wrapper for the socket
        let (stream, pair) = Stream::of(sock);

        debug!("Listener::accept; ~ Sending socket to consumer");

        // Send the sender / stream pair to the user
        let busy = tx.send(pair);

        match busy.poll() {
            Ok(Ok(tx)) => {
                Some((stream, self.ready(tx)))
            }
            Ok(Err(_)) => {
                Some((stream, Action::remove()))
            }
            Err(busy) => {
                self.wait(busy, notify, token);
                Some((stream, Action::wait()))
            }
        }
    }

    fn wait<A>(&mut self, tx: A, notify: &Notify, token: Token)
            where A: Async<Value=Sender<Pair<Bytes>>> {

        // Wait for interest to be registered before attempting to accept
        // from the socket
        let notify = notify.clone();
        tx.receive(move |res| {
            debug!("Listener::wait; socket ready");
            match res {
                Ok(tx) => {
                    if notify.accept_interest(Some(tx), token) {
                        return;
                    }
                }
                Err(_) => {
                    // The consumer has dropped the stream and does not want
                    // any new sockets. The listener can be closed.
                    if notify.accept_interest(None, token) {
                        return;
                    }
                }
            }

            panic!("[unimplemented] failed to notify reactor of ready listener");
        });
    }
}

enum State {
    New { tx: Sender<Pair<Bytes>> },
    Waiting,
    Listening { tx: Sender<Pair<Bytes>> },
}

impl State {
    fn new_to_waiting(&mut self) -> Sender<Pair<Bytes>> {
        match mem::replace(self, State::Waiting) {
            State::New { tx } => tx,
            _ => panic!("unexpected state"),
        }
    }

    fn waiting_to_listening(&mut self, tx: Sender<Pair<Bytes>>) {
        match mem::replace(self, State::Listening { tx: tx }) {
            State::Waiting => {},
            _ => panic!("unexpected state"),
        }
    }

    fn listening_to_waiting(&mut self) -> Sender<Pair<Bytes>> {
        match mem::replace(self, State::Waiting) {
            State::Listening { tx} => tx,
            _ => panic!("unexpected state"),
        }
    }
}

impl fmt::Debug for Listener {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "eventual_io::Listener {{ ... }}")
    }
}
