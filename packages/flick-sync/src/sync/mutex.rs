use std::{
    ops::{Deref, DerefMut},
    time::Instant,
};

use tokio::sync as tsync;
use tracing::debug;

use crate::sync::{MAX_LOCK_TIME, MAX_WAIT_TIME};

pub(crate) struct Mutex<T: ?Sized> {
    name: &'static str,
    lock: tsync::Mutex<T>,
}

impl<T: ?Sized> Mutex<T> {
    pub(crate) fn new(name: &'static str, inner: T) -> Self
    where
        T: Sized,
    {
        Self {
            name,
            lock: tsync::Mutex::new(inner),
        }
    }

    pub(crate) async fn lock(&self) -> MutexGuard<'_, T> {
        let start = Instant::now();
        let guard = self.lock.lock().await;

        let delta = Instant::now() - start;
        if delta > MAX_WAIT_TIME {
            debug!(
                millis = delta.as_millis(),
                mutex = self.name,
                "Mutex locking waited too long"
            )
        }

        MutexGuard {
            name: self.name,
            lock_time: Instant::now(),
            guard,
        }
    }
}

pub(crate) struct MutexGuard<'a, T: ?Sized> {
    name: &'static str,
    lock_time: Instant,
    guard: tsync::MutexGuard<'a, T>,
}

impl<'a, T: ?Sized> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        let delta = Instant::now() - self.lock_time;
        if delta >= MAX_LOCK_TIME {
            debug!(
                millis = delta.as_millis(),
                mutex = self.name,
                "Mutex locked too long"
            )
        }
    }
}

impl<T: ?Sized> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.guard.deref()
    }
}

impl<T: ?Sized> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard.deref_mut()
    }
}
