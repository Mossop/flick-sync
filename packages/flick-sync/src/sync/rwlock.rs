use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
    time::Instant,
};

use std::sync;
use tokio::sync as tsync;
use tracing::debug;

use crate::sync::{MAX_LOCK_TIME, MAX_WAIT_TIME};

struct LockData {
    readers: u64,
    total_readers: u64,
    lock_time: Option<Instant>,
}

struct LockTracker {
    name: &'static str,
    data: Arc<sync::Mutex<LockData>>,
}

impl Drop for LockTracker {
    fn drop(&mut self) {
        let Ok(mut lock_data) = self.data.lock() else {
            return;
        };

        let is_read = lock_data.readers > 0;
        if is_read {
            lock_data.readers -= 1;

            if lock_data.readers > 0 {
                return;
            }
        }

        let Some(lock_time) = lock_data.lock_time.take() else {
            debug!("Invalid lock state, missing lock time");
            return;
        };

        let delta = Instant::now() - lock_time;
        if delta >= MAX_LOCK_TIME {
            if is_read {
                debug!(
                    readers = lock_data.total_readers,
                    millis = delta.as_millis(),
                    rwlock = self.name,
                    "Read locks held too long"
                )
            } else {
                debug!(
                    millis = delta.as_millis(),
                    rwlock = self.name,
                    "Write lock held too long"
                )
            }
        }

        lock_data.total_readers = 0;
    }
}

pub(crate) struct RwLock<T: ?Sized> {
    name: &'static str,
    data: Arc<sync::Mutex<LockData>>,
    lock: Arc<tsync::RwLock<T>>,
}

impl<T: ?Sized> RwLock<T> {
    pub(crate) fn new(name: &'static str, inner: T) -> Self
    where
        T: Sized,
    {
        Self {
            name,
            data: Arc::new(sync::Mutex::new(LockData {
                readers: 0,
                total_readers: 0,
                lock_time: None,
            })),
            lock: Arc::new(tsync::RwLock::new(inner)),
        }
    }

    fn add_reader(&self) -> LockTracker {
        let mut lock_data = self.data.lock().unwrap();
        lock_data.readers += 1;
        lock_data.total_readers += 1;
        if lock_data.lock_time.is_none() {
            lock_data.lock_time = Some(Instant::now());
        }

        LockTracker {
            name: self.name,
            data: self.data.clone(),
        }
    }

    fn add_writer(&self) -> LockTracker {
        let mut lock_data = self.data.lock().unwrap();
        lock_data.lock_time = Some(Instant::now());

        LockTracker {
            name: self.name,
            data: self.data.clone(),
        }
    }

    pub(crate) async fn read(&self) -> RwLockReadGuard<'_, T> {
        let start = Instant::now();
        let guard = self.lock.read().await;

        let delta = Instant::now() - start;
        if delta > MAX_WAIT_TIME {
            debug!(
                millis = delta.as_millis(),
                rwlock = self.name,
                "Read lock waited too long"
            )
        }

        RwLockReadGuard {
            tracker: self.add_reader(),
            guard,
        }
    }

    pub(crate) async fn read_owned(self: Arc<Self>) -> OwnedRwLockReadGuard<T> {
        let start = Instant::now();
        let guard = self.lock.clone().read_owned().await;

        let delta = Instant::now() - start;
        if delta > MAX_WAIT_TIME {
            debug!(
                millis = delta.as_millis(),
                rwlock = self.name,
                "Read lock waited too long"
            )
        }

        OwnedRwLockReadGuard {
            tracker: self.add_reader(),
            guard,
        }
    }

    pub(crate) async fn write(&self) -> RwLockWriteGuard<'_, T> {
        let start = Instant::now();
        let guard = self.lock.write().await;

        let delta = Instant::now() - start;
        if delta > MAX_WAIT_TIME {
            debug!(
                millis = delta.as_millis(),
                rwlock = self.name,
                "Write lock waited too long"
            )
        }

        RwLockWriteGuard {
            tracker: self.add_writer(),
            guard,
        }
    }

    pub(crate) async fn write_owned(self: Arc<Self>) -> OwnedRwLockWriteGuard<T> {
        let start = Instant::now();
        let guard = self.lock.clone().write_owned().await;

        let delta = Instant::now() - start;
        if delta > MAX_WAIT_TIME {
            debug!(
                millis = delta.as_millis(),
                rwlock = self.name,
                "Write lock waited too long"
            )
        }

        OwnedRwLockWriteGuard {
            tracker: self.add_writer(),
            guard,
        }
    }
}

pub(crate) struct RwLockReadGuard<'a, T: ?Sized> {
    #[allow(unused)]
    tracker: LockTracker,
    guard: tsync::RwLockReadGuard<'a, T>,
}

impl<T: ?Sized> Deref for RwLockReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.guard.deref()
    }
}

pub(crate) struct RwLockWriteGuard<'a, T: ?Sized> {
    tracker: LockTracker,
    guard: tsync::RwLockWriteGuard<'a, T>,
}

impl<'a, T: ?Sized> RwLockWriteGuard<'a, T> {
    pub(crate) fn map<F, U: ?Sized>(this: Self, f: F) -> RwLockMappedWriteGuard<'a, U>
    where
        F: FnOnce(&mut T) -> &mut U,
    {
        let guard = tsync::RwLockWriteGuard::map(this.guard, f);
        RwLockMappedWriteGuard {
            tracker: this.tracker,
            guard,
        }
    }
}

impl<T: ?Sized> Deref for RwLockWriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.guard.deref()
    }
}

impl<T: ?Sized> DerefMut for RwLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard.deref_mut()
    }
}

pub(crate) struct RwLockMappedWriteGuard<'a, T: ?Sized> {
    tracker: LockTracker,
    guard: tsync::RwLockMappedWriteGuard<'a, T>,
}

impl<'a, T: ?Sized> RwLockMappedWriteGuard<'a, T> {
    pub(crate) fn map<F, U: ?Sized>(this: Self, f: F) -> RwLockMappedWriteGuard<'a, U>
    where
        F: FnOnce(&mut T) -> &mut U,
    {
        let guard = tsync::RwLockMappedWriteGuard::map(this.guard, f);
        RwLockMappedWriteGuard {
            tracker: this.tracker,
            guard,
        }
    }
}

impl<T: ?Sized> Deref for RwLockMappedWriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.guard.deref()
    }
}

impl<T: ?Sized> DerefMut for RwLockMappedWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard.deref_mut()
    }
}

pub(crate) struct OwnedRwLockReadGuard<T: ?Sized> {
    #[allow(unused)]
    tracker: LockTracker,
    guard: tsync::OwnedRwLockReadGuard<T>,
}

impl<T: ?Sized> Deref for OwnedRwLockReadGuard<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.guard.deref()
    }
}

pub(crate) struct OwnedRwLockWriteGuard<T: ?Sized> {
    #[allow(unused)]
    tracker: LockTracker,
    guard: tsync::OwnedRwLockWriteGuard<T>,
}

impl<T: ?Sized> Deref for OwnedRwLockWriteGuard<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.guard.deref()
    }
}

impl<T: ?Sized> DerefMut for OwnedRwLockWriteGuard<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard.deref_mut()
    }
}
