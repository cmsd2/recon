//! A child protocol with the inbox its indications are collected into.
//!
//! Every composing protocol did the same eleven lines per child: take the inbox out of `self`,
//! borrow the child, call [`Cx::with_child_consuming`], drain what was collected, put the inbox
//! back. Constraint 4 in `CLAUDE.md` said to write that by hand two or three times before removing
//! it; there were sixteen copies when this was written. This is the removal, and it is a struct
//! rather than a macro so that the control flow stays in the parent's own text.
//!
//! The inbox is handed back **by value** rather than drained here, because the parent handles a
//! child's indications with `&mut self` — including, sometimes, by calling `run` again on the same
//! or another child — and a borrow held across that would not compile. [`Child::reclaim`] puts the
//! allocation back so it is reused across events, which is what `tests/alloc_probe.rs` measures.

use crate::store::{KeyedSlot, SeqSlot, Slot};
use crate::{Cx, ProtoCx, Protocol};
use core::convert::Infallible;
use core::ops::{Deref, DerefMut};

/// A protocol owned by another, with the inbox its indications are collected into.
pub struct Child<P: Protocol> {
    proto: P,
    inbox: Vec<P::Ind>,
}

impl<P: Protocol> Child<P> {
    pub fn new(proto: P) -> Self {
        Child { proto, inbox: Vec::new() }
    }

    /// Replace the protocol, keeping the inbox's allocation.
    ///
    /// For a child that is rebuilt while running — the leader-driven consensus replaces its epoch
    /// consensus on every epoch change.
    pub fn replace(&mut self, proto: P) {
        self.proto = proto;
    }

    /// Put the inbox back after the indications `run` returned have been handled.
    pub fn reclaim(&mut self, mut inbox: Vec<P::Ind>) {
        inbox.clear();
        self.inbox = inbox;
    }
}

impl<P> Child<P>
where
    P: Protocol<Meta = Infallible, Entry = Infallible>,
{
    /// Run `f` against the child, forwarding what it sends as `wrap(msg)` and returning what it
    /// indicated for the parent to handle. Hand the returned inbox to [`Child::reclaim`] once done.
    ///
    /// The child is handed no store — see [`Cx::with_child_consuming`]. A child that keeps
    /// something durably is run with [`Child::run_durable`] instead.
    pub fn run<M, I, Me, En>(
        &mut self,
        cx: &mut Cx<'_, M, I, P::Note, Me, En>,
        wrap: impl Fn(P::Msg) -> M,
        f: impl FnOnce(&mut P, &mut ProtoCx<'_, P>),
    ) -> Vec<P::Ind> {
        let mut inbox = core::mem::take(&mut self.inbox);
        let proto = &mut self.proto;
        cx.with_child_consuming(wrap, &mut inbox, |ccx| f(proto, ccx));
        inbox
    }
}

impl<P> Child<P>
where
    P: Protocol<Entry = Infallible>,
{
    /// [`Child::run`], for a child that keeps durable metadata in `slot` of the parent's record.
    ///
    /// See [`Cx::with_durable_child_consuming`] for why the write is one and not two.
    pub fn run_durable<M, I, Me, En>(
        &mut self,
        cx: &mut Cx<'_, M, I, P::Note, Me, En>,
        wrap: impl Fn(P::Msg) -> M,
        slot: Slot<Me, P::Meta>,
        f: impl FnOnce(&mut P, &mut ProtoCx<'_, P>),
    ) -> Vec<P::Ind> {
        let mut inbox = core::mem::take(&mut self.inbox);
        let proto = &mut self.proto;
        cx.with_durable_child_consuming(wrap, &mut inbox, slot, |ccx| f(proto, ccx));
        inbox
    }
}

impl<P: Protocol> Child<P> {
    /// [`Child::run_durable`], for one member of a family of durable children.
    ///
    /// See [`Cx::with_keyed_durable_child_consuming`].
    pub fn run_keyed<M, I, Me, En, K>(
        &mut self,
        cx: &mut Cx<'_, M, I, P::Note, Me, En>,
        wrap: impl Fn(P::Msg) -> M,
        slot: KeyedSlot<Me, P::Meta, K>,
        key: K,
        f: impl FnOnce(&mut P, &mut ProtoCx<'_, P>),
    ) -> Vec<P::Ind>
    where
        P: Protocol<Entry = Infallible>,
    {
        let mut inbox = core::mem::take(&mut self.inbox);
        let proto = &mut self.proto;
        cx.with_keyed_durable_child_consuming(wrap, &mut inbox, slot, key, |ccx| f(proto, ccx));
        inbox
    }

    /// [`Child::run_durable`], for a child that keeps metadata **and appends**.
    ///
    /// See [`Cx::with_durable_child`]: the child's entries go into the parent's one sequence, so
    /// the order between a parent's entry and its child's is real rather than invented at recovery.
    pub fn run_appending<M, I, Me, En>(
        &mut self,
        cx: &mut Cx<'_, M, I, P::Note, Me, En>,
        wrap: impl Fn(P::Msg) -> M,
        slot: Slot<Me, P::Meta>,
        entries: SeqSlot<En, P::Entry>,
        f: impl FnOnce(&mut P, &mut ProtoCx<'_, P>),
    ) -> Vec<P::Ind> {
        let mut inbox = core::mem::take(&mut self.inbox);
        let proto = &mut self.proto;
        cx.with_durable_child(wrap, &mut inbox, slot, entries, |ccx| f(proto, ccx));
        inbox
    }
}

/// Shows the protocol. The inbox is empty between events, and its element type need not be
/// `Debug` for the parent to be.
impl<P: Protocol + core::fmt::Debug> core::fmt::Debug for Child<P> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.proto.fmt(f)
    }
}

impl<P: Protocol> Deref for Child<P> {
    type Target = P;
    fn deref(&self) -> &P {
        &self.proto
    }
}

impl<P: Protocol> DerefMut for Child<P> {
    fn deref_mut(&mut self) -> &mut P {
        &mut self.proto
    }
}
