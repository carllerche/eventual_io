mod listener;
mod stream;

pub use self::listener::Listener;
pub use self::stream::Stream;

#[derive(Debug)]
pub enum Evented {
    Stream(Stream),
    Listener(Listener),
}

impl Evented {
    pub fn listener(&mut self) -> &mut Listener {
        match *self {
            Evented::Listener(ref mut v) => v,
            _ => panic!("expected Evented to be net::Listener"),
        }
    }

    pub fn stream(&mut self) -> &mut Stream {
        match *self {
            Evented::Stream(ref mut v) => v,
            _ => panic!("expected Evented to be net::Stream"),
        }
    }
}

pub enum Action {
    Wait,
    Register(Interest),
    Remove,
}

impl Action {
    pub fn wait() -> Action {
        Action::Wait
    }

    pub fn is_register(&self) -> bool {
        match *self {
            Action::Register(..) => true,
            _ => false,
        }
    }

    pub fn read() -> Action {
        Action::Register(Interest::Read)
    }

    pub fn write() -> Action {
        Action::Register(Interest::Write)
    }

    pub fn read_write() -> Action {
        Action::Register(Interest::ReadWrite)
    }

    pub fn remove() -> Action {
        Action::Remove
    }
}

pub enum Interest {
    Read,
    Write,
    ReadWrite,
}
