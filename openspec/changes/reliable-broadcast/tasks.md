## 1. The protocol

- [x] 1.1 Define the wire type carrying the original sender and a per-sender sequence number, and
      the module documentation quoting Algorithm 3.3; verify the wire round-trips through the codec
- [x] 1.2 Implement eager reliable broadcast over best-effort broadcast — deliver on first receipt
      and relay unconditionally on first receipt; verify unit tests of the handlers' effects
- [x] 1.3 Verify the relay terminates: a relayed message returning to a process that has already
      delivered it produces no further relay

## 2. Its guarantees

- [x] 2.1 Assert validity and no duplication over the simulator under loss and duplication
- [x] 2.2 Assert no creation, including that a relayed message is attributed to its originator and
      not to the relayer
- [x] 2.3 Assert agreement when the sender crashes partway through, across many seeds — the
      property best-effort broadcast cannot provide
- [x] 2.4 Verify the agreement tests are not vacuous: confirm that the same scenario run against
      best-effort broadcast does violate agreement, so the test distinguishes the two rungs

## 3. What it cost

- [x] 3.1 Record whether composing a second transforming layer repeated the perfect link's
      boilerplate verbatim, which is the evidence the deferred macro decision was waiting for
