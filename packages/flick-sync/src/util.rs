use std::{
    collections::HashMap,
    hash::Hash,
    io,
    pin::Pin,
    result,
    task::{Context, Poll},
};

use pin_project::pin_project;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tokio::io::AsyncWrite;

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
