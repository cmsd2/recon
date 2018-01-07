#!/bin/bash

export RUST_LOG="recon=trace,gossip=trace"

trap 'kill $(jobs -pr)' SIGINT SIGTERM EXIT

truncate -s 0 peers.txt

for PORT in `seq 9000 9008`; do
  echo "127.0.0.1:$PORT" >> peers.txt
done

cargo build --example gossip

for PORT in `seq 9000 9008`; do
  ./target/debug/examples/gossip 127.0.0.1:$PORT peers.txt > $PORT.txt 2>&1 &
done

tail -f 9000.txt
