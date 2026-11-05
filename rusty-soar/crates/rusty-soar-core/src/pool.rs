//! Fixed-capacity object pools.
//!
//! A pool allocates all of its slots once, when it is created, and never grows: allocation
//! takes a free slot or fails. Nothing is allocated after initialization, and every loop over
//! a pool is bounded by its capacity, which keeps the engine within fixed memory and makes its
//! loops boundable for Kani.
//!
//! Where the slots live is chosen by a [`Slots`] family: [`HeapSlots`] allocates them once on
//! the heap, for agents of any size; [`InlineSlots`] keeps them in arrays inside the pool, for
//! Kani proofs. Kani models heap memory as untyped bytes, which it tracks far less precisely
//! than typed arrays, so proofs over inline arrays are much smaller. Both families run the same
//! pool code.

use alloc::boxed::Box;
use alloc::vec;
use core::fmt;
use core::ops::{Index, IndexMut};

/// Where a pool's slots are stored.
pub trait Slots {
    /// A fixed-length array of `T`.
    type Array<T>: AsRef<[T]> + AsMut<[T]>;

    /// An array of at least `len` copies of `value`.
    fn filled<T: Clone>(len: usize, value: T) -> Self::Array<T>;
}

/// Slots allocated once on the heap, with exactly the capacity asked for.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeapSlots;

impl Slots for HeapSlots {
    type Array<T> = Box<[T]>;

    fn filled<T: Clone>(len: usize, value: T) -> Box<[T]> {
        vec![value; len].into_boxed_slice()
    }
}

/// Slots in arrays of `N` stored inline, for Kani proofs. A pool asked for more than `N` slots
/// panics when it is created.
#[derive(Debug, Clone, Copy, Default)]
pub struct InlineSlots<const N: usize>;

impl<const N: usize> Slots for InlineSlots<N> {
    type Array<T> = [T; N];

    fn filled<T: Clone>(len: usize, value: T) -> [T; N] {
        assert!(len <= N, "a pool of {} slots does not fit in inline storage of {}", len, N);
        core::array::from_fn(|_| value.clone())
    }
}

/// Where each of an agent's pools keeps its slots: a [`Slots`] family per pool.
///
/// Every [`Slots`] family is a layout that keeps all the pools in it. A Kani proof can define
/// its own layout to size the pools separately: inline storage gives every array of a family
/// the same length, and an agent whose pools are all as large as its largest one can be too
/// big for Kani to build.
pub trait Layout {
    /// The Rete's WMEs.
    type ReteWmes: Slots;
    /// The Rete's alpha memories.
    type AlphaMemories: Slots;
    /// The Rete's alpha-memory entries.
    type AlphaEntries: Slots;
    /// The Rete's beta nodes.
    type Nodes: Slots;
    /// The Rete's join tests.
    type Tests: Slots;
    /// The Rete's disjunction constants.
    type DisjunctionValues: Slots;
    /// The Rete's tokens.
    type Tokens: Slots;
    /// The Rete's negation blocks.
    type Blocks: Slots;
    /// The Rete's productions, and the agent's records of them.
    type Productions: Slots;
    /// The Rete's pending and fired matches.
    type Matches: Slots;
    /// The agent's rule actions and new-identifier letters.
    type Actions: Slots;
}

impl<S: Slots> Layout for S {
    type ReteWmes = S;
    type AlphaMemories = S;
    type AlphaEntries = S;
    type Nodes = S;
    type Tests = S;
    type DisjunctionValues = S;
    type Tokens = S;
    type Blocks = S;
    type Productions = S;
    type Matches = S;
    type Actions = S;
}

/// A pool of `T` with a fixed number of slots.
pub struct Pool<T, S: Slots = HeapSlots> {
    slots: S::Array<Option<T>>,
    /// Free slot indices: the first `free_len` entries, used as a stack whose top
    /// (`free_len - 1`) is handed out next.
    free: S::Array<u32>,
    free_len: usize,
    capacity: usize,
}

impl<T: Clone, S: Slots> Pool<T, S> {
    /// Creates a pool with `capacity` slots, all free.
    pub fn with_capacity(capacity: usize) -> Self {
        let mut free = S::filled(capacity, 0u32);
        // Lowest indices are handed out first.
        for (i, slot) in free.as_mut()[..capacity].iter_mut().enumerate() {
            *slot = (capacity - 1 - i) as u32;
        }
        Self { slots: S::filled(capacity, None), free, free_len: capacity, capacity }
    }
}

impl<T, S: Slots> Pool<T, S> {
    /// Stores `value` in a free slot and returns its index, or `None` when the pool is full.
    pub fn insert(&mut self, value: T) -> Option<usize> {
        self.free_len = self.free_len.checked_sub(1)?;
        let index = self.free.as_ref()[self.free_len] as usize;
        self.slots.as_mut()[index] = Some(value);
        Some(index)
    }

    /// Frees a slot, returning its value.
    pub fn remove(&mut self, index: usize) -> Option<T> {
        let value = self.slots.as_mut()[..self.capacity].get_mut(index)?.take()?;
        self.free.as_mut()[self.free_len] = index as u32;
        self.free_len += 1;
        Some(value)
    }

    /// The value in a slot.
    pub fn get(&self, index: usize) -> Option<&T> {
        self.slots.as_ref()[..self.capacity].get(index)?.as_ref()
    }

    /// The value in a slot, mutably.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.slots.as_mut()[..self.capacity].get_mut(index)?.as_mut()
    }

    /// Occupied slots in index order.
    pub fn iter(&self) -> impl Iterator<Item = (usize, &T)> {
        self.slots.as_ref()[..self.capacity].iter().enumerate().filter_map(|(i, s)| s.as_ref().map(|v| (i, v)))
    }

    /// Number of occupied slots.
    pub fn len(&self) -> usize {
        self.capacity - self.free_len
    }

    /// Whether no slot is occupied.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Total number of slots.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

impl<T, S: Slots> fmt::Debug for Pool<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pool").field("len", &self.len()).field("capacity", &self.capacity).finish()
    }
}

/// Iterator over a [`BoundedVec`]'s items.
pub type Iter<'a, T> = core::iter::Map<core::slice::Iter<'a, Option<T>>, fn(&Option<T>) -> &T>;

/// Mutable iterator over a [`BoundedVec`]'s items.
pub type IterMut<'a, T> = core::iter::Map<core::slice::IterMut<'a, Option<T>>, fn(&mut Option<T>) -> &mut T>;

/// An append-ordered list with a fixed capacity, allocated once.
pub struct BoundedVec<T, S: Slots = HeapSlots> {
    /// The first `len` slots are occupied, in order; the rest are empty.
    items: S::Array<Option<T>>,
    len: usize,
    capacity: usize,
}

impl<T: Clone, S: Slots> BoundedVec<T, S> {
    /// Creates an empty list that can hold `capacity` items.
    pub fn with_capacity(capacity: usize) -> Self {
        Self { items: S::filled(capacity, None), len: 0, capacity }
    }
}

impl<T, S: Slots> BoundedVec<T, S> {
    /// Appends an item, or returns it back when the list is full (it never reallocates).
    pub fn push(&mut self, value: T) -> Result<(), T> {
        if self.len >= self.capacity {
            return Err(value);
        }
        self.items.as_mut()[self.len] = Some(value);
        self.len += 1;
        Ok(())
    }

    /// Removes and returns the item at `index`, shifting later items down. Panics if `index`
    /// is out of range, as `Vec::remove` does.
    pub fn remove(&mut self, index: usize) -> T {
        assert!(index < self.len, "removal index {} is out of range for length {}", index, self.len);
        let items = &mut self.items.as_mut()[..self.len];
        let value = items[index].take().expect("occupied");
        items[index..].rotate_left(1);
        self.len -= 1;
        value
    }

    /// Removes every item.
    pub fn clear(&mut self) {
        for item in &mut self.items.as_mut()[..self.len] {
            *item = None;
        }
        self.len = 0;
    }

    /// Number of items.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the list is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The items in order.
    pub fn iter(&self) -> Iter<'_, T> {
        self.items.as_ref()[..self.len].iter().map(|item| item.as_ref().expect("occupied"))
    }

    /// The items in order, mutably.
    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        self.items.as_mut()[..self.len].iter_mut().map(|item| item.as_mut().expect("occupied"))
    }
}

impl<T, S: Slots> Index<usize> for BoundedVec<T, S> {
    type Output = T;

    fn index(&self, index: usize) -> &T {
        self.items.as_ref()[..self.len][index].as_ref().expect("occupied")
    }
}

impl<T, S: Slots> IndexMut<usize> for BoundedVec<T, S> {
    fn index_mut(&mut self, index: usize) -> &mut T {
        self.items.as_mut()[..self.len][index].as_mut().expect("occupied")
    }
}

impl<T, S: Slots> fmt::Debug for BoundedVec<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoundedVec").field("len", &self.len).field("capacity", &self.capacity).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert_remove_and_exhaustion<S: Slots>() {
        let mut pool: Pool<char, S> = Pool::with_capacity(2);
        let a = pool.insert('a').unwrap();
        let b = pool.insert('b').unwrap();
        assert_eq!((a, b), (0, 1));
        assert!(pool.insert('c').is_none(), "full");
        assert_eq!(pool.remove(a), Some('a'));
        assert_eq!(pool.insert('d'), Some(0), "slot reused");
        assert_eq!(pool.len(), 2);
    }

    #[test]
    fn heap_pool() {
        insert_remove_and_exhaustion::<HeapSlots>();
    }

    #[test]
    fn inline_pool() {
        insert_remove_and_exhaustion::<InlineSlots<2>>();
    }

    #[test]
    fn bounded_vec_keeps_order() {
        let mut list: BoundedVec<u8, InlineSlots<3>> = BoundedVec::with_capacity(3);
        for x in [1, 2, 3] {
            list.push(x).unwrap();
        }
        assert_eq!(list.push(4), Err(4));
        assert_eq!(list.remove(0), 1);
        assert_eq!(list.iter().copied().collect::<alloc::vec::Vec<_>>(), [2, 3]);
        assert_eq!(list[1], 3);
    }
}
