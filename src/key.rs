use std::marker::PhantomData;
use std::cmp::{Eq, PartialEq};
use std::hash::{Hash, Hasher};

// ['IoKey', 'TaskId'] are used to uniquely identify I/O operations and tasks within the single-threaded runtime.
// They are !Send and !Sync

/// Represents an I/O Key.
/// This type is used to uniquely identify I/O operations within the single-threaded runtime.
#[derive(Clone, Copy)]
pub struct IoKey {
    /// The internal identifier.
    pub inner: u32,
    /// A PhantomData marker to enforce thread confinement.
    phantom: PhantomData<*const ()>
}

/// Creates an `IoKey` from a `u32` value.
impl From<u32> for IoKey {
    fn from(value: u32) -> Self {
        Self { inner: value, phantom: PhantomData }
    }
}

impl PartialEq for IoKey {
    /// Checks equality based on the inner value.
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl Eq for IoKey {}

impl Hash for IoKey {
    /// Implements hashing based on the inner value.
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u32(self.inner);
    }
}

impl nohash::IsEnabled for IoKey {}

/// Represents a Task Identifier.
/// Similar to `IoKey`, `TaskId` is used for uniquely identifying tasks in the single-threaded runtime,
/// and is also not transferable across threads.
#[derive(Clone, Copy)]
pub struct TaskId {
    /// The internal identifier.
    pub inner: u32,
    /// A PhantomData marker to enforce thread confinement.
    phantom: PhantomData<*const ()>
}

/// Creates a `TaskId` from a `u32` value.
impl From<u32> for TaskId {
    fn from(value: u32) -> Self {
        Self { inner: value, phantom: PhantomData }
    }
}

impl PartialEq for TaskId {
    /// Checks equality based on the inner value.
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl Eq for TaskId {}

impl Hash for TaskId {
    /// Implements hashing based on the inner value.
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u32(self.inner);
    }
}

impl nohash::IsEnabled for TaskId {}
