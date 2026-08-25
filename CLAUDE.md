# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build                                                  # whole workspace
cargo test                                                   # (only placeholder tests exist today)
cargo test -p recon-link -- it_works                         # single test by name
cargo build -p recon-link --example reconnect                # build the demo
RUST_LOG=recon_link=trace cargo run -p recon-link \
    --example reconnect --features logger                    # run it with logging
```

The `reconnect` example dials `127.0.0.1:6666` and speaks newline-delimited text, so
start something to talk to first (`nc -l 6666`). Kill and restart the listener to
exercise the reconnect path. Logging in the example is gated behind the `logger`
feature (`env_logger`); the library itself always logs through the `log` facade.

## Layout and vintage

Cargo workspace of three crates: `recon-link` (all the real code), `recon-service`
(a few standalone trait definitions), and `recon` (an empty facade crate that just
depends on `recon-link`). `archive/recon-gossip/` is **not** a workspace member — it
is an older tokio-proto/tokio-service gossip prototype kept for reference, with git
dependencies that may no longer resolve. Don't try to build it as part of normal work.

This is **futures 0.1 / tokio-core 0.1 era code on Rust edition 2015** — `Poll`,
`Async`, `AsyncSink`, `try_ready!`, `extern crate`, `Box<Future<...>>` without `dyn`.
It builds on a current toolchain but emits deprecation warnings. Keep new code in the
same style rather than mixing in `async`/`await` or futures 0.3; converting the crate
is a deliberate, whole-crate job, not something to do incidentally.

## recon-link architecture

`conn::Connection` is the whole library. It is a `#[derive(StateMachineFuture)]` enum
(the `state_machine_future` crate), which means much of the API you will use is
generated rather than written:

- each variant (`NotConnected`, `Connecting`, `Connected`) becomes a struct with those fields;
- a `PollConnection` trait is generated with one `poll_<state>` method per variant,
  implemented at the bottom of `conn.rs`;
- `AfterNotConnected` / `AfterConnecting` / `AfterConnected` enums enumerate the legal
  transitions declared in each variant's `#[state_machine_future(transitions(...))]`
  attribute — **adding a transition means editing that attribute too**, or the `.into()`
  conversion won't exist;
- `Connection::start(...)` returns `ConnectionFuture`, wrapped by `Connection::new`.

Four type parameters flow through every state: `S` (local stream of outgoing `Item`),
`K` (local sink receiving `Message<Item>`), `T` (the transport, both `Stream` and
`Sink` of `Item`), and `N: NewTransport<Transport=T>` (the factory used to build a
fresh transport on each connect). The example wires these to a delayed iterator, a
printing sink, `TcpStream` + the line codec from `framing.rs`.

Data flow while `Connected`:

```
local stream S ──▶ outbound: VecDeque<TimestampedItem<Item>> ──▶ tcp sink T
local sink   K ◀── inbound:  VecDeque<Message<Item>>         ◀── tcp stream T
```

### Invariants worth preserving

**Session ids.** `session_id` increments on *every* departure from `Connected` back to
`NotConnected`. On entering `Connected`, a `Message::Control { Event::Connected {
session_id } }` is pushed to the front of the inbound queue, and every subsequent
`Message::Packet` carries that id. This is the crate's delivery contract: within one
session, TCP ordering holds and any suffix of the stream may be lost; a session id bump
is the local side's signal that a gap may have occurred.

**Progress-or-NotReady.** `poll_connected` sets `progress = true` whenever any sub-poll
returned `Ready`, and only returns `Async::NotReady` when nothing progressed. Returning
`NotReady` after making progress stalls the connection; unconditionally returning
`Ready(Connected{..})` spins the reactor forever (this was a real bug — see commit
de2e926). Any new sub-poll added to that function must participate in the flag.

**Bounded queues.** `Config { inbound_max, outbound_max, outbound_max_age }` bounds both
queues; the read loops stop at the cap, which is what applies backpressure. Config is
threaded through every state struct — new config fields must be copied through all of
them.

**Stale-outbound reconnect.** Outbound items are wrapped in `TimestampedItem`. If the
transport says `AsyncSink::NotReady` for an item older than `outbound_max_age`, the
machine tears down the connection and reconnects rather than waiting — the cure for a
half-dead TCP connection that accepts no writes.

**Reconnection policy** lives in `poll_not_connected`: `tokio_retry::Retry::spawn` with
`FibonacciBackoff::from_millis(1).max_delay(2000ms)` plus jitter, wrapping
`N::new_transport()`. Transport read/write errors bump `error_count` and return to
`NotConnected`; a clean end-of-stream from the transport reconnects without incrementing
it. Only the *local* stream ending terminates the future (`Finished`).

### framing.rs

A newline-delimited `String` codec (`ReconFrame::Message`) used by the example. Note
that `decode_eof` deliberately returns `io::Error` (`UnexpectedEof` / `InvalidData`)
instead of a `Done` frame — that error is what drives the state machine back to
`NotConnected`, so it is load-bearing, not an oversight.

## recon-service

`AsyncService` / `NewService` / `Service` traits only; nothing in `recon-link` uses them
yet. Treat it as a design sketch for the layer above the link.
