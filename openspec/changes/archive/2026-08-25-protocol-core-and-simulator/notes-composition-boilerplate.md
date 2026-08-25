# Composition boilerplate, measured

Task 8.1. Constraint 4 of `docs/postmortem.md` says to write two or three protocols by hand
before writing any macro to remove the boilerplate, so that the decision rests on measurement
rather than anticipation. Three are now written. This is what they cost.

## What was measured

Non-blank, non-comment lines. "Ceremony" counts lines inside the `impl Protocol` block that exist
only to arrange composition — the `with_child` calls, the `mem::take` dance, the scratch-buffer
restores, and the local re-borrows the closure requires.

| Protocol | Code | `impl Protocol` | Ceremony | Composition form |
|---|---:|---:|---:|---|
| `stubborn_link` | 69 | 28 | **0** | none — it is the leaf |
| `perfect_link` | 100 | 36 | **12** | `with_child_consuming` ×3 |
| `best_effort_broadcast` | 65 | 27 | **6** | `with_child` ×3 |

## The finding

**The cost is not uniform, and it splits on one question: does the layer transform its child's
indications, or pass them on?**

Best-effort broadcast forwards. Its second handler is Algorithm 3.1's `upon ⟨pl, Deliver | p, m⟩
do trigger ⟨beb, Deliver | p, m⟩` and nothing more, so a free function does the translation and
each handler is one call:

```rust
fn on_msg(&mut self, from: NodeId, msg: pl::Wire<P>, cx: &mut ProtoCx<'_, Self>) {
    let link = &mut self.link;
    cx.with_child(core::convert::identity, forward, Timer::Link, |ccx| {
        link.on_msg(from, msg, ccx)
    });
}
```

Two lines of ceremony per handler, no fields, nothing to explain.

The perfect link transforms: a delivery from the stubborn link is an *input* to deduplication, not
an output. The parent cannot react while the child holds the borrow, so indications are collected
and processed afterwards — which costs an `inbox` field on the struct, a `mem::take` and restore
in each handler, and a `consume_inbox` pass:

```rust
fn on_msg(&mut self, from: NodeId, msg: Wire<P>, cx: &mut ProtoCx<'_, Self>) {
    let stubborn = &mut self.stubborn;
    let mut inbox = core::mem::take(&mut self.inbox);
    cx.with_child_consuming(core::convert::identity, Timer::Stubborn, &mut inbox, |ccx| {
        stubborn.on_msg(from, msg, ccx)
    });
    self.inbox = inbox;
    self.consume_inbox(cx);
}
```

Four lines of ceremony per handler plus a field that is not part of the algorithm.

## Was the earlier change worth it?

Yes, and by more than expected. The first design had `Cx` hold `&mut Vec<Effect<..>>`, which
forced *every* parent to carry a scratch field and do the `mem::take` dance — the shape the
perfect link still has, applied universally. Moving to an `EffectSink` with a mapping adapter took
a toy two-layer parent from 48 lines to 27, and it is why best-effort broadcast has no field at
all. The remaining cost is confined to transforming layers, where it is arguably honest: those
layers really do have to defer.

## Recommendation: still no macro

Three data points, and the repetition is real but small and unevenly distributed:

- **Six lines** of mechanical repetition in the forwarding case, in three near-identical handlers.
- **Twelve lines** plus a field in the transforming case.

A derive could plausibly generate both, since the mappers are variant constructors and the pattern
does not vary. But:

1. **There is not yet enough variation to generalise from.** Two shapes, one instance of each. The
   failure this project documented was building the framework before the thing it frames.
2. **The ceremony is not where the reading difficulty is.** The algorithms read closely against the
   page already; what a reader stumbles over is `mem::take`, and that has a narrower fix than a
   macro — see below.
3. **A macro would hide the borrow structure**, which is the part a reader most needs to see: the
   perfect link defers *because* it cannot hold `&mut self` while its child runs. Concealing that
   would make the code shorter and less true.

**Revisit at reliable broadcast** (rung 5), which will be the second transforming layer. If its
handlers repeat the perfect link's four lines verbatim, that is the second instance a derive needs.

## The narrower fix worth trying first

The `mem::take`/restore pair exists only because `inbox` is a field of the same struct being
borrowed. A scoped accessor on `Cx` that lends a caller-owned buffer for the duration of the child
call would remove all four lines without any macro, and without hiding anything. That is a change
to one function in `recon-core`, not a code generator, and it should be tried before anything is
generated.
