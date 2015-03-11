use core::{self, Bytes, Pair, Sender};
use net::{self, Action};
use mio::{self, EventLoop, Handler, Interest, NonBlock, ReadHint, PollOpt, Token};
use mio::tcp::{TcpListener, TcpStream};
use mio::util::Slab;
use std::io;
use std::sync::Arc;

pub struct Reactor {
    inner: Arc<Inner>,
}

impl Reactor {
    pub fn start() -> io::Result<Reactor> {
        use std::thread;

        let mut event_loop = try!(EventLoop::new());
        let inner = Arc::new(Inner::new(event_loop.channel()));
        let notify = inner.notify.clone();

        thread::spawn(move || {
            let mut handler = IoHandler::new(notify);
            event_loop.run(&mut handler).unwrap();
        });

        Ok(Reactor { inner: inner })
    }

    pub fn stream(&self, io: NonBlock<TcpStream>) -> Pair<Bytes> {
        let (stream, pair) = net::Stream::of(io);

        if !self.inner.notify.stream(stream) {
            panic!("[unimplemented] failed to register stream with reactor");
        }

        pair
    }

    /// Accept connections from the given `TcpListener`
    pub fn accept(&self, io: NonBlock<TcpListener>) -> core::Stream<Pair<Bytes>>  {
        let (listener, rx) = net::Listener::of(io);

        if !self.inner.notify.accept(listener) {
            panic!("[unimplemented] failed to register listener with reactor");
        }

        rx
    }
}

impl Clone for Reactor {
    fn clone(&self) -> Reactor {
        Reactor { inner: self.inner.clone() }
    }
}

struct Inner {
    notify: Notify,
}

impl Inner {
    fn new(sender: mio::Sender<Message>) -> Inner {
        Inner { notify: Notify::new(sender) }
    }
}

/*
 *
 * ===== Notify =====
 *
 */

#[derive(Debug)]
pub enum Message {
    Stream(net::Stream),
    Accept(net::Listener),
    AcceptInterest(Option<Sender<Pair<Bytes>>>, Token),
    ReadInterest(Option<Sender<Bytes>>, Token),
    WriteInterest(Option<(Bytes, core::Stream<Bytes>)>, Token),
}

pub struct Notify {
    sender: mio::Sender<Message>,
}

impl Notify {
    fn new(sender: mio::Sender<Message>) -> Notify {
        Notify { sender: sender }
    }

    pub fn stream(&self, stream: net::Stream) -> bool {
        self.sender.send(Message::Stream(stream)).is_ok()
    }

    pub fn accept(&self, listener: net::Listener) -> bool {
        self.sender.send(Message::Accept(listener)).is_ok()
    }

    pub fn accept_interest(&self, tx: Option<Sender<Pair<Bytes>>>, token: Token) -> bool {
        self.sender.send(Message::AcceptInterest(tx, token))
            .is_ok()
    }

    pub fn stream_read_ready(&self, tx: Option<Sender<Bytes>>, token: Token) -> bool {
        self.sender.send(Message::ReadInterest(tx, token)).is_ok()
    }

    pub fn stream_write_ready(&self, rx: Option<(Bytes, core::Stream<Bytes>)>, token: Token) -> bool {
        self.sender.send(Message::WriteInterest(rx, token)).is_ok()
    }
}

impl Clone for Notify {
    fn clone(&self) -> Notify {
        Notify { sender: self.sender.clone() }
    }
}

/*
 *
 * ===== IoHandler =====
 *
 */

struct IoHandler {
    conns: Slab<net::Evented>,
    notify: Notify,
}

impl IoHandler {
    fn new(notify: Notify) -> IoHandler {
        IoHandler {
            conns: Slab::new(65_535),
            notify: notify,
        }
    }
}

impl IoHandler {
    /*
     *
     * ===== Listener =====
     *
     */

    // Start managing a new listener
    fn listen(&mut self, event_loop: &mut EventLoop<IoHandler>, token: Token) {
        let action = self.conns[token].listener().listen(&self.notify, token);
        self.handle_listener_action(action, event_loop, token);
    }

    // The consumer has expressed interest in the next value or does not want
    // any further values.
    fn accept_interest(&mut self,
                       event_loop: &mut EventLoop<IoHandler>,
                       tx: Option<Sender<Pair<Bytes>>>,
                       token: Token) {

        // If `tx` has a value, then the consumer is ready to accept a new
        // socket, otherwise it is no longer interested in new values.
        match tx {
            Some(tx) => {
                let action = self.conns[token].listener().ready(tx);
                self.handle_listener_action(action, event_loop, token);
            }
            None => {
                debug!("Reactor::accept_interest; closing listener socket");
                // The consumer has dropped the accept stream and is no longer
                // interested in accepting new connections. Close the socket,
                // this is done by dropping the data.
                self.conns.remove(token);
            }
        }
    }

    // Process the requested listener action
    fn handle_listener_action(&mut self,
                              action: Action,
                              event_loop: &mut EventLoop<IoHandler>,
                              token: Token) {

        // Process the listener action
        match action {
            Action::Register(..) => {
                self.listener_register(event_loop, token);
            }
            Action::Remove => {
                debug!("Reactor::handle_listener_action; closing listener socket");
                self.conns.remove(token);
            }
            _ => {}
        }
    }

    // Register the listener's socket with the event loop
    fn listener_register(&mut self, event_loop: &mut EventLoop<IoHandler>, token: Token) {
        debug!("Reactor::listener_register; registering event loop interest");

        let res = event_loop.register_opt(
            self.conns[token].listener().io(),
            token,
            Interest::readable(),
            PollOpt::edge() | PollOpt::oneshot());

        if let Err(_) = res {
            panic!("[unimplemented] failed to register interest with event loop");
        }
    }

    fn accept(&mut self, event_loop: &mut EventLoop<IoHandler>, token: Token) {
        let mut reregister;

        debug!("Reactor::accept; Attempting to accept socket");

        // TODO: Consider looping if consumer is ready to accept another socket
        match self.conns[token].listener().accept(&self.notify, token) {
            Some((stream, action)) => {
                self.stream(event_loop, stream);
                reregister = action.is_register();
            }
            None => reregister = true,
        }

        if reregister {
            self.listener_register(event_loop, token);
        }
    }

    /*
     *
     * ===== Stream =====
     *
     */

    fn stream(&mut self, event_loop: &mut EventLoop<IoHandler>, stream: net::Stream) {
        let token = match self.conns.insert(net::Evented::Stream(stream)) {
            Ok(token) => token,
            Err(_) => panic!("[unimplemented] slab full - send error"),
        };

        let action = self.conns[token].stream().init(&self.notify, token);
        self.handle_stream_action(action, event_loop, token);
    }

    fn read(&mut self, event_loop: &mut EventLoop<IoHandler>, token: Token) {
        let action = self.conns[token].stream().read(&self.notify, token);
        self.handle_stream_action(action, event_loop, token);
    }

    fn write(&mut self, event_loop: &mut EventLoop<IoHandler>, token: Token) {
        let action = self.conns[token].stream().write(&self.notify, token);
        self.handle_stream_action(action, event_loop, token);
    }

    // Process the requested stream action
    fn handle_stream_action(&mut self,
                            action: Action,
                            event_loop: &mut EventLoop<IoHandler>,
                            token: Token) {

        // Process the stream action
        match action {
            Action::Register(interest) => {
                self.stream_register(interest, event_loop, token);
            }
            Action::Remove => {
                debug!("Closing stream socket");
                self.conns.remove(token);
            }
            _ => {}
        }
    }

    fn read_interest(&mut self,
                     event_loop: &mut EventLoop<IoHandler>,
                     sender: Option<Sender<Bytes>>,
                     token: Token) {

        match sender {
            Some(sender) => {
                let action = self.conns[token].stream().read_interest(sender);
                self.handle_stream_action(action, event_loop, token);
            }
            None => {
                let action = self.conns[token].stream().read_close();
                self.handle_stream_action(action, event_loop, token);
            }
        }
    }

    fn write_interest(&mut self,
                      event_loop: &mut EventLoop<IoHandler>,
                      head: Option<(Bytes, core::Stream<Bytes>)>,
                      token: Token) {

        // If there is data, write it and wait for more, otherwise shutdown
        // writes.
        match head {
            Some((bytes, rest)) => {
                let action = self.conns[token].stream().write_interest(bytes, rest);
                self.handle_stream_action(action, event_loop, token);
            }
            None => {
                debug!("Reactor::write_interest; closing stream writes");
                let action = self.conns[token].stream().write_close();
                self.handle_stream_action(action, event_loop, token);
            }
        }
    }

    // Register the listener's socket with the event loop
    fn stream_register(&mut self, interest: net::Interest, event_loop: &mut EventLoop<IoHandler>, token: Token) {
        let ev_interest = match interest {
            net::Interest::Read => Interest::readable(),
            net::Interest::Write => Interest::writable(),
            net::Interest::ReadWrite => Interest::readable() | Interest::writable(),
        };

        let res = event_loop.register_opt(
            self.conns[token].stream().io(),
            token,
            ev_interest,
            PollOpt::edge() | PollOpt::oneshot());

        if let Err(_) = res {
            panic!("[unimplemented] failed to register interest with event loop");
        }
    }
}

impl Handler for IoHandler {
    type Timeout = ();
    type Message = Message;

    fn readable(&mut self, event_loop: &mut EventLoop<IoHandler>, token: Token, _: ReadHint) {
        match self.conns[token] {
            net::Evented::Listener(..) => self.accept(event_loop, token),
            net::Evented::Stream(..) => self.read(event_loop, token),
        }
    }

    fn writable(&mut self, event_loop: &mut EventLoop<IoHandler>, token: Token) {
        // Only stream type sockets are writable, so just do the write
        self.write(event_loop, token);
    }

    fn notify(&mut self, event_loop: &mut EventLoop<IoHandler>, msg: Message) {
        match msg {
            Message::Stream(stream) => {
                self.stream(event_loop, stream);
            }
            Message::Accept(listener) => {
                let token = match self.conns.insert(net::Evented::Listener(listener)) {
                    Ok(token) => token,
                    Err(_) => panic!("[unimplemented] slab full - send error"),
                };

                self.listen(event_loop, token);
            }
            Message::AcceptInterest(tx, token) => {
                self.accept_interest(event_loop, tx, token);
            }
            Message::ReadInterest(tx, token) => {
                self.read_interest(event_loop, tx, token);
            }
            Message::WriteInterest(head, token) => {
                self.write_interest(event_loop, head, token);
            }
        }
    }
}
