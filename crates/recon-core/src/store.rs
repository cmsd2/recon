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
/// # The sequence half does not exist, and this is what it would be
///
/// A slot scopes the **metadata** only, so [`Cx::with_durable_child_consuming`] hands the child a
/// store whose `Entry` is uninhabited: a child that appends cannot be composed, and the signature
/// says so rather than a comment.
///
/// Scoping the sequence is a different shape. The parent's `Entry` would have to be a sum, the slot
/// would carry `fn(CEn) -> En` and `fn(&En) -> Option<&CEn>`, `read_from` would filter to the
/// child's variant, and the [`Position`]s the child saw would be the parent's — sparse, but still
/// ordered and still comparable, which is all a cursor needs. Nothing needs it:
/// `logged_uniform_reliable_broadcast` is the only protocol here that appends, and nothing composes
/// over it. Building it now would be the failure this repository already documented — the framework
/// before its second consumer.
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
