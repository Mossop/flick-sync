use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::Stream;
use pin_project::{pin_project, pinned_drop};
use tracing::{Level, Span, field, span};

pub(crate) fn uniform_title(st: &str) -> String {
    let title = st.to_lowercase();

    title
        .trim()
        .trim_start_matches("a ")
        .trim()
        .trim_start_matches("the ")
        .trim()
        .to_string()
}

#[pin_project(PinnedDrop)]
pub(crate) struct StreamLimiter<S> {
    start_position: u64,
    position: u64,
    limit: u64,
    span: Span,
    #[pin]
    inner: S,
}

impl<S> StreamLimiter<S> {
    pub(crate) fn new(stream: S, start_position: u64, limit: u64) -> Self {
        let span = span!(
            Level::INFO,
            "response_stream",
            "streamed_bytes" = field::Empty,
            "start_position" = start_position,
            "limit" = limit,
        );

        Self {
            start_position,
            position: start_position,
            limit,
            inner: stream,
            span,
        }
    }
}

#[pinned_drop]
impl<S> PinnedDrop for StreamLimiter<S> {
    fn drop(self: Pin<&mut Self>) {
        self.span
            .record("streamed_bytes", self.position - self.start_position);
    }
}

impl<S> Stream for StreamLimiter<S>
where
    S: Stream<Item = Result<Bytes, io::Error>>,
{
    type Item = Result<Bytes, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let _entered = self.span.clone().entered();

        if self.position >= self.limit {
            return Poll::Ready(None);
        }

        let this = self.project();

        match this.inner.poll_next(cx) {
            Poll::Ready(Some(Ok(mut bytes))) => {
                *this.position += bytes.len() as u64;

                if this.limit < this.position {
                    bytes.truncate(bytes.len() - (*this.position - *this.limit) as usize);
                    *this.position = *this.limit;
                }

                Poll::Ready(Some(Ok(bytes)))
            }
            o => o,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = (self.limit - self.position) as usize;
        (len, Some(len))
    }
}
