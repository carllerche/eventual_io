extern crate bytes;
extern crate mio;
extern crate eventual;
extern crate eventual_io;

use mio::tcp;
use eventual::Async;
use eventual_io as eio;

pub fn main() {
    // Create the socket
    let reactor = eio::Reactor::start().unwrap();
    let r = reactor.clone();

    // Create a server socket
    let srv = tcp::listen(&"127.0.0.1:3000".parse().unwrap()).unwrap();

    println!(" + Accepting connections");
    reactor.accept(srv)
        // Process client connections with at most 10 in-flight at any given
        // time.
        .process(10, move |(src_tx, src_rx)| {
            println!(" + Handling socket");

            // Hard coded to a google IP
            let (client, _) = tcp::connect(&"216.58.216.164:80".parse().unwrap()).unwrap();
            let (dst_tx, dst_rx) = r.stream(client);

            let a = dst_tx.send_all(src_rx).map_err(|_| ());
            let b = src_tx.send_all(dst_rx).map_err(|_| ());

            eventual::join((a, b)).and_then(|v| {
                println!(" + Socket done");
                Ok(v)
            })
        })
        .reduce((), |_, _| ()) // TODO: .consume()
        .await().unwrap();
}
