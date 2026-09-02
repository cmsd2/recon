//! Durable state, read and written synchronously.
//!
//! A write does not return until it would survive a crash. So a protocol that writes and then
//! sends cannot be seen to have made a promise it has no record of — there is no other point at
//! which a driver could synchronise with a synchronous protocol.
//!
//! Reads are synchronous too, which is honest while the record is mirrored in memory. For a log
//! larger than memory a read is a real disk read; that is a bound of this interface.

use core::convert::Infallible;

/// A position in the appended sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Position(pub u64);

impl Position {
    pub const START: Position = Position(0);
}

/// One value that is replaced, and a sequence that grows.
///
/// The split is the point: metadata is small and rewritten, entries accumulate. Rewriting
/// something that accumulates costs `O(n²)` over a run, so it must be appended instead.
///
/// A protocol that keeps nothing durably declares both types uninhabited, and then `set` and
/// `append` take an argument that cannot be constructed. Reads stay callable and return nothing.
pub trait Store<Meta, Entry> {
    fn get(&self) -> Option<&Meta>;
    fn set(&mut self, meta: Meta);
    fn append(&mut self, entry: Entry) -> Position;
    fn read_from(&self, from: Position) -> Vec<&Entry>;
    /// One past the last entry.
    fn end(&self) -> Position;
}

/// What a child that keeps nothing durably is handed.
///
/// This is the default, and it is what [`Cx::with_child`](crate::Cx::with_child) and
/// [`Cx::with_child_consuming`](crate::Cx::with_child_consuming) supply. A child that *does* keep
/// something durably is composed through a [`Slot`] instead — see
/// [`Cx::with_durable_child_consuming`](crate::Cx::with_durable_child_consuming).
#[derive(Debug, Default)]
pub struct NoStore;

impl Store<Infallible, Infallible> for NoStore {
    fn get(&self) -> Option<&Infallible> {
        None
    }

    fn set(&mut self, meta: Infallible) {
        match meta {}
    }

    fn append(&mut self, entry: Infallible) -> Position {
        match entry {}
    }

    fn read_from(&self, _from: Position) -> Vec<&Infallible> {
        Vec::new()
    }

    fn end(&self) -> Position {
        Position::START
    }
}

/// Where a child's durable record lives inside its parent's.
///
/// A parent and a child sharing one store would collide on the metadata: each `set` would
/// overwrite the other's. A `Slot` says which part of the parent's record belongs to the child, so
/// the child's `set` becomes a read-modify-write of the parent's — **one record, one write**, which
/// is what keeps durable-before-visible meaning what it says. Two writes could be interrupted
/// between them; one cannot.
///
/// Both halves are `fn` pointers rather than closures, for the same reason the composition mappers
/// are: a slot must not capture. It names a fixed place in a type, and a slot that could close over
/// state would be a different place on different calls.
///
/// `write` takes the parent's record as an `Option` because the child may write first: nothing is
/// stored until something is, and the child's own `Init` is often the first event of the run. The
/// implementation is then "start from the parent's default, put the child's part in it".
///
/// ```
/// # use recon_core::Slot;
/// #[derive(Clone, Default, PartialEq, Debug)]
/// struct Parent { mine: u64, childs: Option<u32> }
///
/// const CHILD: Slot<Parent, u32> = Slot {
///     read: |p| p.childs.as_ref(),
///     write: |p, c| Parent { childs: Some(c), ..p.cloned().unwrap_or_default() },
/// };
///
/// assert_eq!((CHILD.read)(&Parent { mine: 1, childs: Some(7) }), Some(&7));
/// assert_eq!((CHILD.write)(None, 7), Parent { mine: 0, childs: Some(7) });
/// ```
///
/// # The sequence half is [`SeqSlot`]
///
/// A `Slot` scopes the **metadata** only, so [`Cx::with_durable_child_consuming`] hands the child a
/// store whose `Entry` is uninhabited: such a child cannot append, and the signature says so rather
/// than a comment. That is still the right default, and most durable children want nothing else.
///
/// A child that *appends* is composed through [`SeqSlot`] as well, with
/// [`Cx::with_durable_child`](crate::Cx::with_durable_child). This paragraph used to say the shape
/// such a thing would take and that nothing needed it — "building it now would be the framework
/// before its second consumer". The second consumer arrived: the fail-recovery total-order
/// broadcast keeps a durable record of its own *and* composes
/// `logged_uniform_reliable_broadcast`, which is the one protocol here that appends. What was
/// built is what that paragraph described, unchanged.
///
/// [`Cx::with_durable_child_consuming`]: crate::Cx::with_durable_child_consuming
pub struct Slot<Parent, Child> {
    /// The child's record, as it sits inside the parent's.
    pub read: fn(&Parent) -> Option<&Child>,
    /// The parent's record with the child's part replaced.
    pub write: fn(Option<&Parent>, Child) -> Parent,
}

impl<Parent, Child> Clone for Slot<Parent, Child> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Parent, Child> Copy for Slot<Parent, Child> {}

impl<Parent, Child> core::fmt::Debug for Slot<Parent, Child> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Slot")
    }
}

/// A [`Slot`] for an `Option` field of a `Clone + Default` parent record — the common case.
///
/// `slot!(Parent, field)` writes the two projections every such slot writes the same way: read the
/// field, and write the parent with the field replaced, starting from the parent's default if there
/// is no parent record yet.
///
/// ```
/// # use recon_core::{Slot, slot};
/// #[derive(Clone, Default, PartialEq, Debug)]
/// struct Parent { mine: u64, childs: Option<u32> }
///
/// const CHILD: Slot<Parent, u32> = slot!(Parent, childs);
/// assert_eq!((CHILD.write)(None, 7), Parent { mine: 0, childs: Some(7) });
/// assert_eq!((CHILD.read)(&Parent { mine: 1, childs: Some(3) }), Some(&3));
/// ```
#[macro_export]
macro_rules! slot {
    ($parent:ty, $field:ident) => {
        $crate::Slot::<$parent, _> {
            read: |p| p.$field.as_ref(),
            write: |p, c| {
                let mut whole: $parent = p.cloned().unwrap_or_default();
                whole.$field = Some(c);
                whole
            },
        }
    };
}

/// A child's view of one slot of its parent's store.
///
/// Reads project; writes read the parent's record, replace the child's part, and write the whole
/// thing back. The parent's `Entry` type is carried only so this satisfies [`Store`] — nothing here
/// ever touches the sequence.
pub(crate) struct SlotStore<'p, Parent, Child, En> {
    pub(crate) parent: &'p mut dyn Store<Parent, En>,
    pub(crate) slot: Slot<Parent, Child>,
}

impl<Parent, Child, En> Store<Child, Infallible> for SlotStore<'_, Parent, Child, En> {
    fn get(&self) -> Option<&Child> {
        self.parent.get().and_then(self.slot.read)
    }

    fn set(&mut self, child: Child) {
        // One write, not two: the parent's record comes back with the child's part replaced, and
        // goes down as a whole. A protocol above this cannot be interrupted between two writes,
        // because there is only one.
        let whole = (self.slot.write)(self.parent.get(), child);
        self.parent.set(whole);
    }

    fn append(&mut self, entry: Infallible) -> Position {
        match entry {}
    }

    fn read_from(&self, _from: Position) -> Vec<&Infallible> {
        Vec::new()
    }

    fn end(&self) -> Position {
        Position::START
    }
}

/// Where a child's appended entries live inside its parent's sequence.
///
/// [`Slot`]'s counterpart, and the same idea one type along: the parent's `Entry` is a sum, the
/// child's entries are one of its variants, and both append into **one** sequence rather than two.
/// One sequence is what keeps the ordering between a parent's entry and its child's real — two
/// would have no order between them at all, and a recovery replaying them would be inventing one.
///
/// `wrap` puts a child's entry into the parent's vocabulary; `project` takes it back out and says
/// `None` for an entry that is not the child's.
///
/// # The positions the child sees are the parent's
///
/// The store handed to such a child filters the parent's sequence to the child's variant, so what the child
/// reads back is its own entries in order — but the [`Position`]s are the parent's, and therefore
/// **sparse**: a child's third entry may sit at position seven. That is deliberate and is all a
/// cursor needs, since positions are only ever compared and advanced, never counted. A child that
/// treated a position as an index into its own entries would be wrong, and would have been wrong
/// about a plain store too.
///
/// `fn` pointers rather than closures, for the reason [`Slot`] gives: a slot names a fixed place in
/// a type, and one that could close over state would name a different place on different calls.
pub struct SeqSlot<Entry, ChildEntry> {
    /// A child's entry, in the parent's vocabulary.
    pub wrap: fn(ChildEntry) -> Entry,
    /// The child's entry inside a parent's, or `None` if this entry is not the child's.
    pub project: fn(&Entry) -> Option<&ChildEntry>,
}

impl<Entry, ChildEntry> Clone for SeqSlot<Entry, ChildEntry> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Entry, ChildEntry> Copy for SeqSlot<Entry, ChildEntry> {}

impl<Entry, ChildEntry> core::fmt::Debug for SeqSlot<Entry, ChildEntry> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SeqSlot")
    }
}

/// Where one of a *family* of children keeps its record, inside its parent's.
///
/// [`Slot`] names a fixed place, which is right when a parent has one child of a kind. A parent
/// holding a family — one instance per round, per epoch, per slot of a log — needs a place *per
/// member*, and the member is not known when the slot is written.
///
/// The key is **data, not a capture**. `read` and `write` stay `fn` pointers and take the key as an
/// argument, so a keyed slot still names one fixed function; what varies is what it is applied to.
/// That is what [`Slot`]'s own note means by "a slot must not capture": a slot closing over state
/// would be a *different* function on different calls, and this is the same function every time.
///
/// The parent is responsible for the keyspace being a keyspace. Two children handed the same key
/// share a record, exactly as two `Slot`s naming one field would.
pub struct KeyedSlot<Parent, Child, K> {
    /// The child's record for `key`, as it sits inside the parent's.
    pub read: for<'a> fn(&'a Parent, &K) -> Option<&'a Child>,
    /// The parent's record with the child's part for `key` replaced.
    pub write: fn(Option<&Parent>, &K, Child) -> Parent,
}

impl<Parent, Child, K> Clone for KeyedSlot<Parent, Child, K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Parent, Child, K> Copy for KeyedSlot<Parent, Child, K> {}

impl<Parent, Child, K> core::fmt::Debug for KeyedSlot<Parent, Child, K> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("KeyedSlot")
    }
}

/// What one member of a family of durable children is handed.
pub(crate) struct KeyedSlotStore<'p, Parent, Child, K, En> {
    pub(crate) parent: &'p mut dyn Store<Parent, En>,
    pub(crate) slot: KeyedSlot<Parent, Child, K>,
    pub(crate) key: K,
}

impl<Parent, Child, K, En> Store<Child, Infallible> for KeyedSlotStore<'_, Parent, Child, K, En> {
    fn get(&self) -> Option<&Child> {
        self.parent.get().and_then(|p| (self.slot.read)(p, &self.key))
    }

    fn set(&mut self, child: Child) {
        // One write, as with a plain slot: the parent's whole record comes back with this member's
        // part replaced.
        let whole = (self.slot.write)(self.parent.get(), &self.key, child);
        self.parent.set(whole);
    }

    fn append(&mut self, entry: Infallible) -> Position {
        match entry {}
    }

    fn read_from(&self, _from: Position) -> Vec<&Infallible> {
        Vec::new()
    }

    fn end(&self) -> Position {
        Position::START
    }
}

/// What a child that keeps metadata **and** appends is handed: both halves of its parent's record,
/// scoped.
pub(crate) struct FullSlotStore<'p, Parent, Child, En, CEn> {
    pub(crate) parent: &'p mut dyn Store<Parent, En>,
    pub(crate) slot: Slot<Parent, Child>,
    pub(crate) entries: SeqSlot<En, CEn>,
}

impl<Parent, Child, En, CEn> Store<Child, CEn> for FullSlotStore<'_, Parent, Child, En, CEn> {
    fn get(&self) -> Option<&Child> {
        self.parent.get().and_then(self.slot.read)
    }

    fn set(&mut self, child: Child) {
        let whole = (self.slot.write)(self.parent.get(), child);
        self.parent.set(whole);
    }

    fn append(&mut self, entry: CEn) -> Position {
        self.parent.append((self.entries.wrap)(entry))
    }

    fn read_from(&self, from: Position) -> Vec<&CEn> {
        self.parent.read_from(from).into_iter().filter_map(self.entries.project).collect()
    }

    fn end(&self) -> Position {
        self.parent.end()
    }
}

/// A store held in memory: the simulator's, and a test's.
#[derive(Debug, Clone)]
pub struct MemStore<Meta, Entry> {
    meta: Option<Meta>,
    entries: Vec<Entry>,
}

impl<Meta, Entry> Default for MemStore<Meta, Entry> {
    fn default() -> Self {
        MemStore { meta: None, entries: Vec::new() }
    }
}

impl<Meta, Entry> MemStore<Meta, Entry> {
    /// Nothing written yet — what distinguishes a first start from a restart.
    pub fn is_empty(&self) -> bool {
        self.meta.is_none() && self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl<Meta, Entry> Store<Meta, Entry> for MemStore<Meta, Entry> {
    fn get(&self) -> Option<&Meta> {
        self.meta.as_ref()
    }

    fn set(&mut self, meta: Meta) {
        self.meta = Some(meta);
    }

    fn append(&mut self, entry: Entry) -> Position {
        let at = Position(self.entries.len() as u64);
        self.entries.push(entry);
        at
    }

    fn read_from(&self, from: Position) -> Vec<&Entry> {
        self.entries.iter().skip(from.0 as usize).collect()
    }

    fn end(&self) -> Position {
        Position(self.entries.len() as u64)
    }
}
