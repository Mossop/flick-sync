use std::{
    collections::HashMap,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::PathBuf,
    pin::Pin,
    result,
    sync::{Arc, Mutex as StdMutex},
    task::{Context, Poll},
    time::Duration,
};

use lazy_static::lazy_static;
use pin_project::pin_project;
use tokio::{
    fs,
    io::{AsyncRead, AsyncSeek, ReadBuf},
    sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock},
    time::timeout,
};
use tracing::trace;

type Lock = Arc<RwLock<()>>;

lazy_static! {
    static ref LOCKS: StdMutex<HashMap<String, (Lock, usize)>> = StdMutex::new(HashMap::new());
}

pub struct Timeout;

async fn attempt<F, R>(fut: F) -> Result<R, Timeout>
where
    F: Future<Output = R>,
{
    timeout(Duration::from_secs(2), fut)
        .await
        .map_err(|_| Timeout)
}

/// A lock for something with a fixed unique key.
pub(crate) struct OpMutex;

impl OpMutex {
    pub(crate) async fn try_lock_write_key(key: String) -> Result<OpWriteGuard, Timeout> {
        let lock = Self::get_or_create(key.clone());

        attempt(lock.write_owned())
            .await
            .map(|guard| OpWriteGuard {
                key: key.clone(),
                guard,
            })
            .inspect_err(|_| {
                trace!(key, "Timed out acquiring write lock");
            })
    }

    pub(crate) async fn try_lock_read_key(key: String) -> Result<OpReadGuard, Timeout> {
        let lock = Self::get_or_create(key.clone());

        attempt(lock.read_owned())
            .await
            .map(|guard| OpReadGuard {
                key: key.clone(),
                guard,
            })
            .inspect_err(|_| {
                trace!(key, "Timed out acquiring read lock");
            })
    }

    fn get_or_create(key: String) -> Lock {
        LOCKS
            .lock()
            .unwrap()
            .entry(key.clone())
            .and_modify(|(_, count)| *count += 1)
            .or_insert_with(|| (Lock::default(), 1))
            .0
            .clone()
    }
}

pub(crate) struct OpWriteGuard {
    key: String,
    #[expect(unused)]
    guard: OwnedRwLockWriteGuard<()>,
}

impl Drop for OpWriteGuard {
    fn drop(&mut self) {
        let mut locks = LOCKS.lock().unwrap();
        let (_, count) = locks.get_mut(&self.key).unwrap();

        if *count > 1 {
            *count -= 1;
        } else {
            locks.remove(&self.key);
        }
    }
}

pub(crate) struct OpReadGuard {
    key: String,
    #[expect(unused)]
    guard: OwnedRwLockReadGuard<()>,
}

impl Drop for OpReadGuard {
    fn drop(&mut self) {
        let mut locks = LOCKS.lock().unwrap();
        let (_, count) = locks.get_mut(&self.key).unwrap();

        if *count > 1 {
            *count -= 1;
        } else {
            locks.remove(&self.key);
        }
    }
}

pub struct LockedFile {
    guard: OpReadGuard,
    path: PathBuf,
}

impl LockedFile {
    pub(crate) fn new<P: ToOwned<Owned = PathBuf>>(path: P, guard: OpReadGuard) -> Self {
        Self {
            guard,
            path: path.to_owned(),
        }
    }

    pub fn file_name(&self) -> &str {
        self.path.file_stem().unwrap().to_str().unwrap()
    }

    pub async fn try_clone(&self) -> Result<Self, Timeout> {
        Ok(Self {
            guard: OpMutex::try_lock_read_key(self.guard.key.clone()).await?,
            path: self.path.clone(),
        })
    }

    pub async fn len(&self) -> result::Result<u64, io::Error> {
        Ok(fs::metadata(&self.path).await?.len())
    }

    pub fn read(self) -> result::Result<LockedFileRead, io::Error> {
        Ok(LockedFileRead {
            guard: self.guard,
            file: File::open(&self.path)?,
        })
    }

    pub async fn async_read(self) -> result::Result<LockedFileAsyncRead, io::Error> {
        Ok(LockedFileAsyncRead {
            guard: self.guard,
            file: fs::File::open(self.path).await?,
        })
    }
}

pub struct LockedFileRead {
    #[expect(unused)]
    guard: OpReadGuard,
    file: File,
}

impl Read for LockedFileRead {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(buf)
    }
}

impl Seek for LockedFileRead {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.file.seek(pos)
    }
}

#[pin_project]
pub struct LockedFileAsyncRead {
    guard: OpReadGuard,
    #[pin]
    file: fs::File,
}

impl AsyncRead for LockedFileAsyncRead {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.project().file.poll_read(cx, buf)
    }
}

impl AsyncSeek for LockedFileAsyncRead {
    fn start_seek(self: Pin<&mut Self>, position: SeekFrom) -> io::Result<()> {
        self.project().file.start_seek(position)
    }

    fn poll_complete(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        self.project().file.poll_complete(cx)
    }
}
