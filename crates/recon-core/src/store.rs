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

/// What a child is handed, so only a child keeping nothing durably can be composed.
///
/// A parent and child sharing one store would collide on the metadata and interleave in the
/// sequence; scoping one store into two is a design nothing yet needs.
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
