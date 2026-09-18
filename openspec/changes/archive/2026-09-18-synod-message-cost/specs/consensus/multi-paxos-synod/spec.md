## ADDED Requirements

### Requirement: The cost of a run is an identity, not an upper bound

The messages a run puts on the wire SHALL be a stated function of the membership, the number of
entries decided and the number of leadership changes, and that function SHALL be asserted rather
than described.

Phase one costs one request and one reply per acceptor **per leadership change**; phase two costs
one request and one reply per acceptor **per entry**, and one decision to every other process. Phase
one being amortised across every entry a leader decides is the whole difference between this
capability and one consensus instance per entry, so it SHALL be visible in the assertion rather than
folded into an average over a run.

An upper bound does not discharge this. A capability sending several times what it needs at a
constant rate satisfies both "the send rate does not grow" and any bound loose enough to hold under
retransmission, which is how sending three times too much went unnoticed.

#### Scenario: A settled run costs exactly what the algorithm needs

- **WHEN** leadership settles and a known number of entries is decided over a run that loses nothing
- **THEN** the count of each message kind equals the stated function of the membership, the entries
  and the leadership changes

#### Scenario: Phase one is paid per leadership change and not per entry

- **WHEN** one leader decides many entries
- **THEN** the number of phase-one exchanges is a function of the leadership changes alone, and does
  not grow with the entries decided

### Requirement: Retransmission is timed against the delivery bound and prompted by the scope

A request SHALL NOT be resent before the time in which an answer could have arrived has passed. The
interval at which outstanding work is swept SHALL NOT by itself determine how often a request is
resent.

Retransmitting sooner than an answer can arrive spends messages on a fault that has not happened.
Over a link that does not lose messages within a session, that is most of what a fixed tick does:
the tick was a stubborn link's idiom, where the network may drop anything at any time, and this
capability does not run over one.

Where the link reports that a scope with a peer has been **established**, outstanding requests to
that peer SHALL be resent at once, and only to that peer. A session ending is the only way this
stack loses a message and the establishment that follows is the only moment a resend can succeed, so
it is the event that recovery is owed to. Waiting out a timeout instead makes recovery slower than
the information available, and it is why the timeout may otherwise be generous.

The threshold SHALL exceed one round trip and SHALL remain below the point at which an attempt is
escalated, and the module SHALL state both bounds. An escalation that fires before a retransmission
has been tried restarts a phase for a message that was merely in flight.

#### Scenario: Nothing is resent before an answer could have arrived

- **WHEN** a request is outstanding for less than the time a round trip takes
- **THEN** it is not resent, however often outstanding work is swept

#### Scenario: A session establishment resends what that peer owes

- **WHEN** a scope with a peer ends while a request to it is outstanding, and a scope with that peer
  is then established
- **THEN** the request is resent to that peer at once, and to no other peer

#### Scenario: Recovery from a lost request does not wait out the timeout

- **WHEN** a request is lost at a scope ending and a scope is established again well before the
  retransmission threshold would elapse
- **THEN** the exchange completes without waiting for the threshold

### Requirement: A message a process addresses to itself is not a message

Where this capability would send to the process it is running on, it SHALL invoke the handler
directly rather than putting the message on the wire.

The roles are co-located — one process holds both the acceptor and the leader — so a request from a
leader to its own acceptor is a function call that the wire has no part in. Counting it as a message
overstates what a deployment costs.

It also removes a fault that cannot happen: a message on the wire can be lost when a scope ends, and
a process cannot fail to deliver a message to itself. A run in which one is dropped is exercising
something no deployment can do.

#### Scenario: Nothing a process sends to itself reaches the wire

- **WHEN** a run decides entries and changes leadership
- **THEN** no message in the run is addressed by a process to itself

#### Scenario: A process still acts on what it addresses to itself

- **WHEN** a leader commands a slot and is itself one of the acceptors
- **THEN** its own acceptor takes up the ballot and answers, and its answer counts toward the
  majority exactly as a remote acceptor's would
