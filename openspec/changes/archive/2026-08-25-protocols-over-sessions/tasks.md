## 1. Two session events, not one

- [x] 1.1 Establish a session as soon as one is possible, without waiting for anything to be sent,
      optionally after a delay modelling backoff; verify a healed partition and a restarted process
      each reconnect with nothing sent from above
- [x] 1.2 Report a session establishment as an event alongside the ending, naming the epoch now in
      force; verify both processes are told
- [x] 1.3 Change the ending to name the epoch that ended rather than predicting the next; verify
      the two reports are distinguishable and carry the epochs they should
- [x] 1.4 Verify a message sent in response to an establishment is delivered — the reason the two
      are separate — by ending a session under partition, healing, and answering the establishment
- [x] 1.5 Verify no establishment is reported for a peer that never returns, and update the session
      link's existing tests, keeping the safety properties among them untouched: a lost suffix is
      still never reported as delivered

## 2. Best-effort broadcast over sessions

- [x] 2.1 Implement fan-out over a session link, reporting a session change upward rather than
      absorbing it; verify its own state holds nothing but the process set as messages grow
- [x] 2.2 Assert validity while sessions hold, and no duplication or creation across a run
- [x] 2.3 Verify a message lost to a session ending is not retried, and that the loss is visible to
      the layer above rather than silent

## 3. Reliable broadcast over sessions

- [x] 3.1 Implement eager reliable broadcast over the layer from group 2, propagating a session
      change; verify a first receipt delivers and relays and a repeat does neither
- [x] 3.2 Assert agreement while sessions hold, including when the original sender crashed
- [x] 3.3 Verify the scoped limit is real: find a schedule where a relay is lost to a session
      ending and a correct process never delivers — the stated limit, demonstrated rather than
      asserted

## 4. Uniform reliable broadcast over sessions

- [x] 4.1 Implement it over the layer from group 2 and the failure detector, adding only the
      re-broadcast clause; verify no new message type appears on the wire and no state is kept
      beyond `pending`, `ack`, `delivered` and `correct`
- [x] 4.2 Assert validity and uniform agreement across session endings and re-establishment
- [x] 4.3 Verify the resend path: a partition well inside the detection timeout resolves because
      what the peer missed is sent again, with the peer still in `correct` throughout
- [x] 4.4 Verify the accusation path: a partition well outside the detection timeout resolves
      because the peer leaves `correct`, with no resend having reached it
- [x] 4.5 Verify nothing is attempted on the ending, where there is no session to send over, and
      that a resend goes only to the peer whose session came back
      **Amended.** The first half as written — nothing resent once the peer has acknowledged
      everything pending — is not a property this algorithm has, and asserting it would have
      required keeping the unsound filter that produced it. See "The resend filter was wrong"
      in `notes.md`.
- [x] 4.6 Verify liveness does not depend on the detector's heartbeats being sends: with the layer
      above sending nothing after a partition heals, the link reconnects on its own, the
      establishment is reported, and the resend happens

## 5. That the two rungs differ

- [x] 5.1 Run reliable and uniform reliable broadcast through the same schedule — a relay lost to a
      session ending — and verify the first can leave a correct process without the message while
      the second cannot
- [x] 5.2 Verify the difference is attributable: the uniform version resolves by resend or by
      accusation, and the reliable version has neither mechanism available to it
- [x] 5.3 Verify the agreement assertions are not vacuous, by asserting minimum delivery counts
      alongside every absence-of-violation property

## 6. What it cost

- [x] 6.1 Record whether separate modules read as copies in practice, how much each genuinely
      differs from its original, and whether that vindicates the decision against a generic layer
- [x] 6.2 Record what moving the report cost, and whether the two liveness paths proved separable
      in testing or overlapped; deliver as notes in the change
