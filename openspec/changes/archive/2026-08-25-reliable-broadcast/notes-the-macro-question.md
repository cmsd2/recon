# The macro question, answered

Task 3.1. The previous change deferred the decision on generating composition boilerplate until a
second transforming layer existed, on the grounds that two shapes with one instance each is not
enough variation to generalise from. Reliable broadcast is that second instance.

**The answer is no macro. Extract a private helper method per composing protocol.**

## What happened

Reliable broadcast was written with the ceremony collected into one private method, `with_beb`,
so each of its three handlers is a single line:

```rust
fn on_msg(&mut self, from: NodeId, msg: Wire<P>, cx: &mut ProtoCx<'_, Self>) {
    self.with_beb(cx, |beb, ccx| beb.on_msg(from, msg, ccx));
}
```

The perfect link had the same ceremony repeated inline across three handlers. Applying the same
treatment to it — a `with_stubborn` method — required no new abstraction and no change to
`recon-core`:

| Protocol | `impl Protocol` before | after |
|---|---:|---:|
| `perfect_link` | 36 | **20** |
| `reliable_broadcast` | — | **17** |
| `best_effort_broadcast` | 27 | 27 (forwards; already one call per handler) |

Sixty-five protocol tests passed unchanged through the refactor.

## Why a helper and not a macro

**It is not less code than a macro would give.** It is roughly the same. The reasons are the other
three.

**The borrow structure stays visible.** The helper is eight lines in the same file, and reading it
shows exactly why the layer defers: it cannot hold `&mut self` while its child runs, so
indications are collected and processed after the child call returns. A derive would make that
disappear, and it is the single thing about this code a reader most needs to understand.

**Each helper differs in the part that matters.** `with_stubborn` deduplicates by identifier;
`with_beb` delivers and then re-enters the child to relay. The ceremony is identical; the handling
is not, and it is the handling that carries the algorithm. A macro would have to be parameterised
by exactly the part that varies, at which point it is generating four lines of scaffolding around
a body the author writes anyway.

**Nothing forces uniformity that should not be uniform.** Best-effort broadcast needs no helper —
it forwards, so `with_child` already gives one call per handler. A macro applied uniformly would
have wrapped it too, adding indirection for nothing.

## The narrower fix, reconsidered

The previous notes proposed a scoped buffer accessor on `Cx` to remove the `mem::take`/restore
pair. With the helper pattern that pair now appears **once per protocol** rather than once per
handler, so the change would save two lines per protocol. That is no longer worth an addition to
the core API. Recommend dropping it.

## What would reopen this

A layer that owns *several* children — which is what uniform reliable broadcast and consensus will
need, since they track state per peer as well as per message. If that produces a helper per child
with the same body three times over, this decision is worth revisiting with that evidence.
