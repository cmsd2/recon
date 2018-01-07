# Recon

Asynchronous messaging on tokio-rs

## Description

Tokio provides low-level building blocks to use mio with futures-rs.
The related crates tokio-service and tokio-proto are designed around
synchronous client/server applications.

This project is designed to provide the building blocks for
implementing asynchronous messaging algorithms between nodes.

## Components

### recon-link

This is an abstraction that sits on top of a TcpStream.
It will aggressively connect and re-connect to a SocketAddr to maintain
a working open connection.

Bytes are sent and received using a Stream and a Sink pair which work with
chunks of Vec<u8>.

Message delivery is not guaranteed. Messages will be dropped when the tcp
session is restarted.

Within a single tcp session messages will arrive in order as per Tcp's
message ordering guarantees.
