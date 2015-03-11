extern crate bytes;
extern crate eventual;
extern crate mio;

#[macro_use]
extern crate log;

pub mod frame;

mod net;
mod reactor;

pub use reactor::Reactor;

/*
 *
 * ===== Convenience type definitions =====
 *
 */

// Private prelude
mod core {
    pub use {
        Complete,
        Error,
        Future,
        Next,
        Pair,
        Result,
        Stream,
        Sender,
    };

    pub use bytes::{ByteStr, Bytes};
    pub use eventual::{Async, AsyncError};
    pub use ::eventual as async;
}

use std::result;

/// Error returned by io ops
pub type Error = ();

pub type Result<T> = result::Result<T, eventual::AsyncError<Error>>;
pub type Future<T> = eventual::Future<T, Error>;
pub type Stream<T> = eventual::Stream<T, Error>;
pub type Sender<T> = eventual::Sender<T, Error>;
pub type Complete<T> = eventual::Complete<T, Error>;
pub type Pair<T> = (Sender<T>, Stream<T>);
pub type Next<T> = Result<Option<(T, Stream<T>)>>;
