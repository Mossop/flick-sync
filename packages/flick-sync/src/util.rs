use std::{
    collections::HashMap,
    collections::VecDeque,
    future::Future,
    hash::Hash,
    io,
    path::Path,
    pin::Pin,
    result,
    task::{Context, Poll},
};

use pin_project::pin_project;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::to_string_pretty;
use tokio::io::{AsyncWrite, AsyncWriteExt};

pub(crate) trait ListItem<T> {
    fn id(&self) -> T;
}

macro_rules! derive_list_item {
    ($typ:ident) => {
        impl ListItem<String> for $typ {
            fn id(&self) -> String {
                self.id.clone()
            }
        }
    };
}

pub(crate) use derive_list_item;
use tracing::trace;

pub(crate) fn from_list<'de, D, K, V>(deserializer: D) -> result::Result<HashMap<K, V>, D::Error>
where
    D: Deserializer<'de>,
    K: Hash + Eq,
    V: ListItem<K> + Deserialize<'de>,
{
    Ok(Vec::<V>::deserialize(deserializer)?
        .into_iter()
        .map(|v| (v.id(), v))
        .collect())
}

pub(crate) fn into_list<S, K, V>(
    map: &HashMap<K, V>,
    serializer: S,
) -> result::Result<S::Ok, S::Error>
where
    S: Serializer,
    V: Serialize,
{
    let list: Vec<&V> = map.values().collect();
    list.serialize(serializer)
}

pub(crate) async fn safe_write<S: Serialize>(
    path: impl AsRef<Path>,
    data: &S,
) -> anyhow::Result<()> {
    let st = to_string_pretty(&data)?;
    let path = path.as_ref();
    let Some(file_name) = path.file_name() else {
        return Ok(tokio::fs::write(path, st).await?);
    };

    let mut temp_path = path.to_owned();
    let mut file_name = file_name.to_owned();
    file_name.push(".temp");
    temp_path.set_file_name(file_name);

    let mut file = tokio::fs::File::create(&temp_path).await?;
    file.write_all(st.as_ref()).await?;
    file.sync_all().await?;
    drop(file);

    Ok(tokio::fs::rename(temp_path, path).await?)
}

pub(crate) fn safe<S: AsRef<str>>(str: S) -> String {
    str.as_ref()
        .chars()
        .map(|x| match x {
            '#' | '%' | '{' | '}' | '\\' | '/' | '<' | '>' | '*' | '?' | '$' | '!' | '"' | '\''
            | ':' | '@' | '+' | '`' | '|' | '=' => '_',
            _ => x,
        })
        .collect()
}

#[pin_project]
pub(crate) struct AsyncWriteAdapter<W> {
    #[pin]
    inner: W,
}

impl<W> AsyncWriteAdapter<W> {
    pub(crate) fn new(inner: W) -> Self {
        Self { inner }
    }
}

impl<W> futures::AsyncWrite for AsyncWriteAdapter<W>
where
    W: AsyncWrite,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.project();
        this.inner.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.project();
        this.inner.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.project();
        this.inner.poll_shutdown(cx)
    }
}

#[pin_project]
struct Parallel<T>
where
    T: Future<Output = ()>,
{
    pending: VecDeque<Pin<Box<T>>>,
    #[pin]
    running: Vec<Pin<Box<T>>>,
    jobs: usize,
}

pub(crate) async fn parallelize<J, T>(futures: J, jobs: usize)
where
    J: IntoIterator<Item = T>,
    T: Future<Output = ()>,
{
    let pending: VecDeque<Pin<Box<T>>> = futures.into_iter().map(Box::pin).collect();

    trace!(jobs, count = pending.len(), "Spawning parallel jobs");

    Parallel {
        pending,
        running: Vec::new(),
        jobs,
    }
    .await
}

impl<T> Future for Parallel<T>
where
    T: Future<Output = ()>,
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();

        while this.running.len() < *this.jobs {
            let Some(next) = this.pending.pop_front() else {
                break;
            };

            this.running.push(next);
        }

        let mut index = 0;
        while index < this.running.len() {
            if this.running[index].as_mut().poll(cx).is_ready() {
                if let Some(next) = this.pending.pop_front() {
                    this.running[index] = next;
                } else {
                    this.running.swap_remove(index);
                }
            } else {
                index += 1;
            }
        }

        if this.pending.is_empty() && this.running.is_empty() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::util::parallelize;

    use futures::task::noop_waker;
    use std::{
        future::Future,
        pin::Pin,
        sync::{Arc, Mutex},
        task::{Context, Poll, Waker},
    };

    #[derive(Default)]
    struct TestState {
        started: Vec<usize>,
        ready: bool,
        waker: Option<Waker>,
    }

    struct TestFuture {
        index: usize,
        state: Arc<Mutex<TestState>>,
    }

    impl Future for TestFuture {
        type Output = ();

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let mut state = self.state.lock().unwrap();

            if !state.started.contains(&self.index) {
                state.started.push(self.index);
            }

            if state.ready {
                Poll::Ready(())
            } else {
                state.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }

    #[test]
    fn limits_parallelism_and_backfills_slots() {
        let states = (0..4)
            .map(|_| Arc::new(Mutex::new(TestState::default())))
            .collect::<Vec<_>>();
        let futures = states.iter().enumerate().map(|(index, state)| TestFuture {
            index,
            state: Arc::clone(state),
        });

        let mut in_parallel = Box::pin(parallelize(futures, 2));
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);

        assert_eq!(in_parallel.as_mut().poll(&mut cx), Poll::Pending);
        assert_eq!(states[0].lock().unwrap().started, vec![0]);
        assert_eq!(states[1].lock().unwrap().started, vec![1]);
        assert!(states[2].lock().unwrap().started.is_empty());
        assert!(states[3].lock().unwrap().started.is_empty());

        {
            let mut state = states[0].lock().unwrap();
            state.ready = true;
            if let Some(waker) = state.waker.take() {
                waker.wake();
            }
        }

        assert_eq!(in_parallel.as_mut().poll(&mut cx), Poll::Pending);
        assert_eq!(states[2].lock().unwrap().started, vec![2]);
        assert!(states[3].lock().unwrap().started.is_empty());

        {
            let mut state = states[1].lock().unwrap();
            state.ready = true;
            if let Some(waker) = state.waker.take() {
                waker.wake();
            }
        }

        assert_eq!(in_parallel.as_mut().poll(&mut cx), Poll::Pending);
        assert_eq!(states[3].lock().unwrap().started, vec![3]);

        for state in &states[2..] {
            let mut state = state.lock().unwrap();
            state.ready = true;
            if let Some(waker) = state.waker.take() {
                waker.wake();
            }
        }

        assert_eq!(in_parallel.as_mut().poll(&mut cx), Poll::Ready(()));
    }
}
