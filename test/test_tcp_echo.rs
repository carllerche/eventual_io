use addr;
use bytes::{Bytes, ByteStr, ToBytes};
use mio::{tcp, Socket};
use eio::{Reactor, Pair, Future};
use eio::frame::{self, Frame};
use eventual::{self, Async};
use std::sync::mpsc;

#[test]
pub fn test_tcp_echo() {
    ::env_logger::init().unwrap();

    let addr = addr::localhost();

    let reactor = Reactor::start().unwrap();

    // Open server socket
    let srv = tcp::v4().unwrap();
    srv.set_reuseaddr(true).unwrap();
    srv.bind(&addr).unwrap();

    let sock = srv.listen(256).unwrap();

    /*
     *
     * ===== Server =====
     */
    let server = reactor.accept(sock)
        .take(1)
        .process(1, |(tx, rx)| {
            debug!("GOT A SOCKET");

            rx.reduce_async(tx, |tx, chunk| {
                debug!("Received a chunk! {:?}", chunk);
                // Echo it back
                tx.send(chunk)
            })
            .and_then(|v| {
                debug!("SERVER CONNECTION DONE");
                Ok(v)
            })
        })
        .and_then(|v| {
            debug!("SERVER DONE");
            Ok(v)
        });

    let (sock, _) = tcp::connect(&addr).unwrap();

    // Initiate the connection
    let pipe = reactor.stream(sock);

    /*
     *
     * ===== Client =====
     *
     */
    fn echo_message(msg: Bytes, (tx, rx): Pair<Bytes>,
                    sender: mpsc::Sender<Bytes>) -> Future<(Pair<Bytes>, mpsc::Sender<Bytes>)> {

        let len = msg.len();
        let expect = msg.clone();

        // Send the message
        let busy = tx.send(msg);
        let s = sender.clone();

        // Frame the response
        let rest = rx.frame_one(frame::Len::new(len))
            .and_then(move |head| {
                let (actual, rest) = head.expect("unexpected EOF");
                assert_eq!(actual, expect);

                debug!(" !!! CLIENT GOT CHUNK !!!");

                // Send the received chunk
                s.send(actual).unwrap();
                Ok(rest)
            });

        let busy = busy.and_then(|tx| {
            debug!("BUSY FUTURE DONE");
            Ok(tx)
        });

        let rest = rest.and_then(|rest| {
            debug!("REST DONE");
            Ok(rest)
        });

        debug!("WAITING FOR BUSY / REST JOIN");
        eventual::join((busy, rest)).and_then(|v| {
            debug!("CLIENT DONE");
            Ok((v, sender))
        })
    }

    let (send, recv) = mpsc::channel();

    let client = echo_message(b"Mary had a little lamb".to_bytes(), pipe, send)
        .and_then(|(pipe, send)| echo_message(b"its fleece was white as snow".to_bytes(), pipe, send))
        .map(drop);

    // Do work
    eventual::join((server, client)).await().unwrap();

    debug!("DONE!");

    let vals: Vec<Bytes> = recv.iter().collect();

    assert_eq!(vals.len(), 2);
    assert_eq!(vals[0], b"Mary had a little lamb".to_bytes());
    assert_eq!(vals[1], b"its fleece was white as snow".to_bytes());
}

    /*
Mary had a little lamb,
its fleece was white as snow;
And everywhere that Mary went,
the lamb was sure to go.

It followed her to school one day,
which was against the rule;
It made the children laugh and play,
to see a lamb at school.
     */
