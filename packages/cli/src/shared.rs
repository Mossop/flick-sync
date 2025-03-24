use std::{
    cmp,
    io::{self, SeekFrom},
    pin::Pin,
    str::FromStr,
    task::{Context, Poll},
};

use actix_web::{
    HttpRequest, HttpResponse, HttpResponseBuilder,
    body::SizedStream,
    http::{
        StatusCode,
        header::{self, ByteRangeSpec, HeaderMap},
    },
};
use pin_project::pin_project;
use tokio::io::{AsyncRead, AsyncSeek, AsyncSeekExt, ReadBuf};
use tokio_util::io::ReaderStream;

const BUFFER_CAPACITY: usize = 8 * 1024;

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

#[pin_project]
pub(crate) struct ByteRangeResponse<R> {
    #[pin]
    reader: R,
    remaining: u64,
}

impl<R> AsyncRead for ByteRangeResponse<R>
where
    R: AsyncRead,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.project();

        if *this.remaining == 0 {
            return Poll::Ready(Ok(()));
        }

        let count = buf.filled().len();
        let result = this.reader.poll_read(cx, buf);

        if let Poll::Ready(Ok(())) = result {
            let new_count = buf.filled().len();
            let filled = (new_count - count) as u64;

            if filled > 0 {
                if *this.remaining < filled {
                    buf.set_filled(*this.remaining as usize);
                    *this.remaining = 0;
                } else {
                    *this.remaining -= filled;
                }
            }
        }

        result
    }
}

impl<R> ByteRangeResponse<R>
where
    R: AsyncRead + AsyncSeek + Unpin + 'static,
{
    fn get_range(request: &HttpRequest) -> Option<ByteRangeSpec> {
        // Only supports the specific case of a single byte range request.
        let range_value = request.headers().get(header::RANGE)?;
        let range_str = range_value.to_str().ok()?;
        let range = header::Range::from_str(range_str).ok()?;

        if let header::Range::Bytes(spec) = range {
            if spec.len() == 1 {
                spec.into_iter().next()
            } else {
                None
            }
        } else {
            None
        }
    }

    fn build_response(status: StatusCode, headers: HeaderMap) -> HttpResponseBuilder {
        let mut response = HttpResponse::build(status);
        response.append_header((header::ACCEPT_RANGES, "bytes"));

        for (key, value) in headers {
            response.append_header((key, value));
        }

        response
    }

    fn build_stream(reader: R, length: u64) -> SizedStream<ReaderStream<ByteRangeResponse<R>>> {
        SizedStream::new(
            length,
            ReaderStream::with_capacity(
                Self {
                    reader,
                    remaining: length,
                },
                BUFFER_CAPACITY,
            ),
        )
    }

    pub(crate) async fn build(
        request: &HttpRequest,
        content_size: u64,
        mut reader: R,
        headers: HeaderMap,
    ) -> HttpResponse {
        let Some(range_spec) = Self::get_range(request) else {
            return Self::build_response(StatusCode::OK, headers)
                .body(Self::build_stream(reader, content_size));
        };

        let (start, length) = match range_spec {
            ByteRangeSpec::FromTo(start, end) => {
                if start >= content_size {
                    return HttpResponse::RangeNotSatisfiable()
                        .append_header(header::ContentRange(header::ContentRangeSpec::Bytes {
                            range: None,
                            instance_length: Some(content_size),
                        }))
                        .finish();
                }

                reader.seek(SeekFrom::Start(start)).await.unwrap();
                let end = cmp::min(end, content_size - 1);
                (start, end - start + 1)
            }
            ByteRangeSpec::From(start) => {
                if start >= content_size {
                    return HttpResponse::RangeNotSatisfiable()
                        .append_header(header::ContentRange(header::ContentRangeSpec::Bytes {
                            range: None,
                            instance_length: Some(content_size),
                        }))
                        .finish();
                }

                reader.seek(SeekFrom::Start(start)).await.unwrap();
                (start, content_size - start)
            }
            ByteRangeSpec::Last(length) => {
                let length = cmp::min(length, content_size);
                (content_size - length, length)
            }
        };

        Self::build_response(StatusCode::PARTIAL_CONTENT, headers)
            .append_header(header::ContentRange(header::ContentRangeSpec::Bytes {
                range: Some((start, start + length - 1)),
                instance_length: Some(content_size),
            }))
            .body(Self::build_stream(reader, length))
    }
}
