# Scope-annotated modules

*An extension to the module notation of Cachin, Guerraoui & Rodrigues, and an argument that it
adds expressiveness without disturbing any existing result.*

The engineering motivation is in [`conditional-guarantees.md`](conditional-guarantees.md). This
document is the argument: what the extension means precisely, why every proof in the book remains
a proof, what is genuinely gained, and — stated as plainly as the rest — what is not.

---

## 1. The model, as the book has it

Fix a set of processes Π. An **execution** σ is an infinite sequence of steps, indexed by ℕ; each
step carries an event and the resulting state. Write `σ|I` for the restriction of σ to an
interval I ⊆ ℕ.

A **module** M is an interface (a set of request and indication events) together with a finite
list of properties. An **algorithm** A *implements* M *using* modules N₁ … Nⱼ. The book's
correctness statement is:

> for every execution σ of A in which the properties of N₁ … Nⱼ hold, the properties of M hold.

Two observations about this, both of which matter later.

**(i) The book's proofs are already conditional.** They are conditional on *which* modules are
assumed, not on *when* those modules' guarantees are in force. Assumption is all-or-nothing over
the whole execution.

**(ii) The book's properties are naturally interval-indexed even though the notation is not.**
"No message is delivered more than once" is a statement about a stretch of time; the notation
simply always means the whole of it. This is what makes the extension possible without
re-founding anything.

Accordingly, take properties to be predicates on an execution *and an interval*:

> **Definition 1 (Property).** A property P is a predicate P(σ, I), read "P holds of σ throughout
> I". A CGR property is recovered as P(σ, [0,∞)).

The three properties of Module 2.3 read directly in this form:

```
PL1(σ, I)  if p sends m to q within I, and p and q are correct throughout I,
           then q delivers m within I
PL2(σ, I)  no message is delivered by a process more than once within I
PL3(σ, I)  every delivery within I of m with sender p is preceded within I
           by p sending m
```

---

## 2. Scopes

> **Definition 2 (Scope).** A scope S assigns to each execution σ a partition `I_S(σ)` of ℕ into
> consecutive intervals, whose boundaries are marked by events of the interface. The event marking
> the end of an interval is written `end(S)`.

The requirement that boundaries be *marked by interface events* is not decoration. A scope whose
ending cannot be observed cannot be reacted to, cannot be tested, and cannot appear in a proof
obligation discharged by an implementation. Section 8 returns to this.

It also constrains *which* module may use a given scope.

> **Definition 2a (Well-formedness).** A module may tag a property with scope S only if the ends
> of S are determined by that module's own interface and state. A tag naming a scope the module
> cannot detect is not merely inconvenient; the obligations of §6 cannot be discharged by any
> implementation of it.

Well-formedness is what keeps a scope annotation from becoming a way to smuggle requirements into
a layer that has no means to meet them. Theorem 8 shows the condition has teeth.

> **Definition 3 (Scoped property).** σ ⊨ P[S]  ⟺  ∀ I ∈ I_S(σ). P(σ, I).

> **Definition 4 (The trivial scope).** `always` is the scope with I_always(σ) = {[0,∞)} for
> every σ.

Scopes are ordered by refinement.

> **Definition 5 (Refinement).** S ⊑ S′ iff every interval of I_S(σ) is contained in some interval
> of I_S′(σ), for every σ.

The scopes of interest form a chain, from the shortest-lived to the longest:

```
    session  ⊑  incarnation  ⊑  always
```

A process restarting ends every session it held, so each session interval lies inside an
incarnation interval; and every interval lies inside [0,∞).

---

## 3. The extension is conservative

> **Theorem 1 (Conservativity).** For every property P and execution σ:
> σ ⊨ P[always] ⟺ P(σ, [0,∞)).
>
> *Proof.* By Definition 3, σ ⊨ P[always] iff P(σ, I) for every I ∈ I_always(σ). By Definition 4
> that set is the singleton {[0,∞)}. ∎

> **Corollary 1.1.** Let M be any module of the book and M[always] the same module with every
> property tagged `[always]` and no scope declared. Then M and M[always] denote the same set of
> executions. Hence for any algorithm A, A ⊨ M iff A ⊨ M[always], and **any proof of the one is,
> without alteration, a proof of the other.**

This is deliberately trivial. A conservative extension that required an argument would not be one.
The content of Theorem 1 is that the annotation is *inert by default*: an unannotated development
is an annotated development in which every tag is `always`, and nothing in the book needs
revisiting, restating, or re-proving. The extension can only be wrong about modules that choose to
use it.

---

## 4. What is actually gained

It would be an overstatement to claim new expressive power in the set-theoretic sense, and the
argument is stronger without it.

> **Proposition 2 (No new predicates).** For every P and S there is a plain property P′ with
> σ ⊨ P[S] ⟺ P′(σ, [0,∞)).
>
> *Proof.* Take P′(σ, ·) ≡ ∀ I ∈ I_S(σ). P(σ, I). ∎

So anything sayable with a scope tag was already sayable in prose. Three things are nonetheless
gained, and they are the reasons the notation is worth its cost.

**(a) A construction, rather than a fresh sentence each time.** P[S] is built systematically from
an unannotated P and a scope S. Proposition 2 guarantees a P′ exists; it offers no assurance that
two authors writing P′ by hand would write the same predicate, or that either matches the
implementation. The tag names the construction.

**(b) A syntactic link between a property and the module's own interface.** This is the real
addition. In the book's notation there is no construct relating a property's validity to any event
of the module that declares it: properties are execution-global, and the interface is a separate
list. Definition 2 requires the scope boundary to be an interface event, so `PL2 [incarnation(q)]`
*names* the event after which PL2 must be re-established. Proofs manipulate syntax, and this is
new syntax.

**(c) Separation of implementations the book's notation conflates.**

> **Proposition 3 (Separation).** There are algorithms A₁ and A₂ over the same interface such that
> A₁ ⊨ PL2[always] and A₂ ⊭ PL2[always], while A₂ ⊨ PL2[incarnation].

*Witnesses.* A₁ is a perfect link whose deduplication set is never lost. A₂ is the same algorithm
on a process that may crash and recover, its deduplication set being volatile. In the book's
notation both are offered as implementations of Module 2.3, and A₂'s claim is false: after a
restart it delivers again a message it had already delivered. With annotation both claims are
true and different.

This is not hypothetical. In this repository the separation is executable: the tests
`no_duplication_does_not_survive_the_recipient_restarting` and
`no_duplication_holds_across_a_suspension` are exactly A₂'s two cases, and they distinguish a
crash that destroys state from a suspension that does not.

---

## 5. Safety and liveness behave differently under scoping

> **Definition 6.** P is *subinterval-closed* if P(σ, I) and J ⊆ I imply P(σ, J).

> **Lemma 4 (Monotonicity for safety).** If P is subinterval-closed and S ⊑ S′, then
> P[S′] ⟹ P[S].
>
> *Proof.* Let I ∈ I_S(σ). By Definition 5, I ⊆ I′ for some I′ ∈ I_S′(σ). From P[S′] we have
> P(σ, I′), and by subinterval-closure P(σ, I). ∎

Safety properties of the book's kind are subinterval-closed: if no message is delivered twice in
a stretch, none is delivered twice in any part of it; likewise no creation. Liveness properties
are not. PL1 promises delivery *eventually within the interval*, and a shorter interval may end
first.

> **Corollary 4.1.** For a safety property, moving to a finer scope is free — the tag can always
> be weakened without proof. For a liveness property it is not, and the gap is precisely the work
> an implementation must do to bridge a scope boundary.

This asymmetry explains the shape of the whole sequence. Every abstraction that "strengthens a guarantee" is
strengthening the *scope of a liveness property*, and pays for it in retained state.

---

## 6. Composition

Let A implement M using N, where N provides Q[S_N] and A claims P[S_M]. Write `state_A(σ, t)` for
A's state, and call a state **admissible** at the start of an interval if the invariants of A's
correctness argument hold there.

> **Rule PROPAGATE.** To establish A ⊨ P[S_M] where S_M = S_N = S:
>
> 1. *(base)* state_A is admissible at the start of every I ∈ I_S(σ);
> 2. *(step)* for every I ∈ I_S(σ), Q(σ, I) implies P(σ, I).

> **Theorem 5 (Soundness of PROPAGATE).** If (1) and (2) hold, A ⊨ P[S].
>
> *Proof.* Definition 3 quantifies over exactly the intervals of I_S(σ); (2) discharges each, and
> (1) supplies the premise each application needs. ∎

The point of Theorem 5 is what obligation (2) *is*: the book's own unannotated proof, read on the
segment σ|I instead of on σ. No new reasoning is required, only a restriction of the old. Obligation
(1) is the genuinely new one, and it is what an implementation discharges by resetting or
re-establishing state at a boundary.

> **Rule BRIDGE.** To establish A ⊨ P[S_M] where S_N ⊏ S_M — that is, A promises over longer
> intervals than its sub-module guarantees:
>
> 1. PROPAGATE, giving P on each I ∈ I_S_N(σ);
> 2. *(stitching)* for each boundary t between consecutive intervals of I_S_N(σ), whatever P
>    demands of a segment spanning t is satisfied within the interval following t.

> **Theorem 6 (Soundness of BRIDGE).** If (1) and (2) hold, A ⊨ P[S_M].
>
> *Proof.* Let J ∈ I_S_M(σ). By S_N ⊏ S_M, J is a union of consecutive intervals of I_S_N(σ)
> separated by boundaries t₁ < t₂ < … Any violation of P(σ, J) is either contained in one of those
> intervals, contradicting (1), or spans some tᵢ, contradicting (2). ∎

By Corollary 4.1, obligation (2) is vacuous for subinterval-closed properties and is the whole
content of the rule for liveness ones. "Bridging a session" is exactly stitching PL1 across
reconnection: resending what was promised and not yet delivered.

---

## 7. Bridging has a price, and the price is provable

The engineering rule of thumb — *a layer can bridge a scope only if its redundancy outlives that
scope* — is a theorem, by the standard indistinguishability argument.

> **Theorem 7 (Bridging requires surviving state).** Let S be a scope whose ending at time t
> destroys a state component X of A. Suppose P[S_M] with S ⊏ S_M requires A's behaviour after t to
> depend on information K determined by σ|[0,t). If K is recoverable only from X, no algorithm
> over A's interface satisfies P[S_M].
>
> *Proof.* Suppose A did. Choose executions σ₁, σ₂ agreeing on every event after t and on every
> component of state surviving t, but with K(σ₁) ≠ K(σ₂); such a pair exists because K is
> determined only by X, which does not survive. A's behaviour after t is a function of its
> surviving state and its subsequent inputs, which coincide in σ₁ and σ₂; so A behaves identically
> after t in both. By hypothesis P[S_M] demands behaviour depending on K, hence different
> behaviour in σ₁ and σ₂. Contradiction. ∎

> **Corollary 7.1 (The sequence, derived).** Take K to be the set of messages promised and not yet
> delivered.
>
> - If K is held in volatile memory, A can bridge `session` but not `incarnation`. *(A perfect
>   link may resend across a reconnect; it cannot across its own restart.)*
> - If K is held in stable storage, A can bridge `incarnation`. *(The book's logged links.)*
> - If K is held at other processes, A can bridge the failure of this one. *(Reliable broadcast:
>   the redundancy is the other processes' copies, which is why it survives the sender's crash and
>   best-effort broadcast does not.)*

Corollary 7.1 is the table in `conditional-guarantees.md`, obtained here as a consequence rather
than as an observation.

> **Corollary 7.2 (Tightness).** `PL2 [incarnation]` is the strongest tag a perfect link with a
> volatile deduplication set can carry. Taking K to be the set of already-delivered identifiers
> and X to be that set, Theorem 7 forbids `PL2 [always]`. Note that the incarnation here is the
> *delivering* process's own: the set that would have to survive is its own, and the boundary is
> its own `⟨Init⟩`.

Theorem 7 bounds what a layer can *repair*. A second bound, in the same style, limits what a layer
can even *notice* — and it is the reason Definition 2a is a restriction rather than a formality.

> **Theorem 8 (Detection requires durable identity).** Let L be a layer whose interface carries
> only messages exchanged with a peer q and the establishment and loss of transport sessions with
> q. Then L cannot distinguish an execution in which q's session was re-established from one in
> which q restarted, unless q's messages carry an identifier that is monotonic across q's restarts.
>
> *Proof.* Suppose no such identifier is carried. Construct σ₁ in which the session with q is lost
> and re-established while q retains its state, and σ₂ in which q crashes, restarts, and a session
> is established with the fresh q. Choose them to agree on every event at L's interface: the same
> session loss, the same re-establishment, and the same subsequent messages — possible because
> nothing in those events is a function of q's state. L's behaviour is a function of its interface
> events and its own state, which coincide; so L behaves identically. Hence L does not distinguish
> them. ∎

> **Corollary 8.1 (The link is the wrong layer).** `PeerRestarted` is not a well-formed scope
> boundary for a point-to-point link. By Theorem 8 it could be detected only from an incarnation
> identifier supplied by the peer; by Theorem 7 that identifier must survive the peer's own
> incarnation, so it requires stable storage at the peer and a handshake to convey it — neither of
> which is in a link's interface. A link may therefore report that a session ended, and nothing
> more, because a session ending is all it observes.

> **Corollary 8.2 (Why the upper layers carry epochs).** A layer that must distinguish a restarted
> peer has to supply the identifier itself: persisted at each process, or agreed among them. This
> is what view numbers in viewstamped replication, terms in Raft, and incarnation numbers in
> gossip membership protocols are. They are not transport facts that the upper layers relay; they
> exist at that level precisely because Theorem 8 forbids obtaining them from below.

---

## 8. Fairness, without which the liveness tags are hollow

If a scope ends often enough, P[S] can hold vacuously: every interval may be too short for
anything to be required of it. A session that dies every millisecond satisfies "delivers within
the session" by never lasting long enough to owe a delivery.

Liveness under scoping therefore needs an assumption of the same kind the field already makes
elsewhere:

> **Assumption F (Eventual stability).** For each scope S used in a liveness tag, I_S(σ) contains
> an interval long enough for the required event to occur.

This is the partial-synchrony global stabilisation time, and the ◇ of an eventually-accurate
failure detector, in the same clothes. Naming it as an explicit assumption is preferable to
leaving it implicit — which, in the unannotated notation, is what happens.

---

## 9. What this does not claim

- **It is not a new logic.** Definition 3 is ordinary interval-indexed reasoning. Nothing here
  needs a semantics the book does not already have, which is the point of §3.
- **It adds no expressive power in the sense of Proposition 2**, and the argument does not rest on
  pretending otherwise. What it adds is a construction, a syntactic link to the interface, and the
  separation of Proposition 3.
- **It decides nothing new.** No verification problem becomes tractable that was not.
- **It requires scopes to be observable *by the module that names them*.** Definitions 2 and 2a
  insist boundaries are interface events of the declaring module. A scope that module cannot
  observe cannot be discharged by any implementation of it nor exercised by a test, and tagging
  with one would be a claim to knowledge the layer has no means to hold — Corollary 8.1 is the
  worked case.
- **It says nothing about probabilistic or partial scope endings** — a session degrading rather
  than ending, a process partially losing state. Both are real and both are outside this.
- **The composition rules assume a refinement chain.** Theorems 5 and 6 use S_N ⊑ S_M. Scopes that
  interleave without refining are not covered, and it is not currently clear whether a useful rule
  exists for them.

---

## 10. Worked example: Module 2.3 annotated

```
Module 2.3′: Perfect point-to-point links

Scopes:
    session(q)     transport session with q; ends when the session is lost
    incarnation    this process's volatile state; ends at its own ⟨Init⟩
                   session(q) ⊑ incarnation ⊑ always

Events:
    Request:    ⟨ pl, Send | q, m ⟩
    Indication: ⟨ pl, Deliver | p, m ⟩
    Indication: ⟨ pl, SessionChanged | q, e ⟩     ends session(q)

Properties:
    PL1 [session(q)]   Reliable delivery
    PL2 [incarnation]  No duplication
    PL3 [always]       No creation

Bridges:    session(q), by retaining unacknowledged messages in memory (Theorem 7,
            first case) and resending after SessionChanged — discharging BRIDGE's
            stitching obligation for PL1.
Propagates: nothing. The incarnation boundary is this process's own ⟨Init⟩; there is
            nobody to notify, because the process that would have raised the event
            is the one that ceased to exist.
```

Three properties, three tags, and the reason for each is a theorem rather than a preference. PL3
is `[always]` by Lemma 4, being subinterval-closed and holding globally. PL1 is `[session]` because
bridging it further would require stitching the book does not perform. PL2 is `[incarnation]` and
tight by Corollary 7.2.

Note what the module does **not** declare. There is no `PeerRestarted`, and no scope naming the
peer's incarnation. By Corollary 8.1 a link cannot observe one, so by Definition 2a it may not tag
anything with it; and a link that reported it anyway would be asserting something it has no means
to know. The most a link can say is that a session ended. A layer needing more must, by
Corollary 8.2, obtain it from persistence or from agreement — which is what the epoch, view and
term numbers of the higher protocols are for.

The book's Module 2.3 is the special case in which every tag reads `always` and the two extra
indications are absent — which by Corollary 1.1 is the book's module exactly, with the book's
proofs unchanged.
