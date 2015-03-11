# Eventual IO

An async IO library built with
[Eventual](https://github.com/carllerche/eventual) futures & streams and
[Mio](https://github.com/carllerche/mio) IO reactor.

** This is in an experimental state **

This should mostly work as is, but error handling is not as robust as it
should be. Also, this should be integrated with a thread pool for
execution and not execute callbacks on the event loop.
