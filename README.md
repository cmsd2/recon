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
It will persistently connect and re-connect to a SocketAddr to maintain a working open connection.

Messages are sent and received using a Stream and a Sink pair locally and remotely. A framed TcpStream works with the remote side.

Message delivery is not guaranteed: within a tcp session any suffix of the stream of Messages may be dropped.

Within a single tcp session messages will arrive in order as per Tcp's message ordering guarantees.

Upon reconnection a control message is sent to the local side with an incremented session id. Messages sent to the local side are tagged with the session id.
