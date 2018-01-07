# Recon

Asynchronous messaging on tokio-rs

## Description

Tokio provides low-level building blocks to use mio with futures-rs.
The related crates tokio-service and tokio-proto are designed around
synchronous client/server applications.

This project is designed to provide the building blocks for
implementing asynchronous messaging algorithms between nodes.

An example implementation of a gossip protocol is included.

## Running the examples

### Gossip

The included bash script creates a peers.txt file with a list of ip addresses and ports.
It then launches multiple copies of the gossip example program which form a mesh
network and begin exchanging messages every 2 seconds.

Messages are broadcast with a fan-out factor k, in a number of rounds.
Messages are stored for resending with probability alpha.
When a later message is received than expected, indicating a missing message,
a resend request is broadcast.
After waiting for a timeout delta, if the resend request was not fulfilled,
the missing message is skipped over.
Messages are delivered in order.

```
./gossip.sh
```

## Warning 

Tokio is still evolving and occasionally makes breaking changes.
Also this is extremely early in development and may not amount to
anything.

## Compatability

For best results use Linux.
Tokio/mio on windows seems to behave a bit differently e.g. with not noticing when a tcp connection is closed.
Not tested on Darwin. 