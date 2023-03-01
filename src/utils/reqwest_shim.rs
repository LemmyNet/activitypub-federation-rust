use crate::error::Error;
use bytes::{BufMut, Bytes, BytesMut};
use futures_core::{ready, stream::BoxStream, Stream};
use pin_project_lite::pin_project;
use reqwest::Response;
use serde::de::DeserializeOwned;
use std::{
    future::Future,
    marker::PhantomData,
    mem,
    pin::Pin,
    task::{Context, Poll},
};

/// 100KB
const MAX_BODY_SIZE: usize = 102400;

pin_project! {
    pub struct BytesFuture {
        #[pin]
        stream: BoxStream<'static, reqwest::Result<Bytes>>,
        limit: usize,
        aggregator: BytesMut,
    }
}

impl Future for BytesFuture {
    type Output = Result<Bytes, Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            let this = self.as_mut().project();
            if let Some(chunk) = ready!(this.stream.poll_next(cx))
                .transpose()
                .map_err(Error::other)?
            {
                this.aggregator.put(chunk);
                if this.aggregator.len() > *this.limit {
                    return Poll::Ready(Err(Error::ResponseBodyLimit));
                }

                continue;
            }

            break;
        }

        Poll::Ready(Ok(mem::take(&mut self.aggregator).freeze()))
    }
}

pin_project! {
    pub struct JsonFuture<T> {
        _t: PhantomData<T>,
        #[pin]
        future: BytesFuture,
    }
}

impl<T> Future for JsonFuture<T>
where
    T: DeserializeOwned,
{
    type Output = Result<T, Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let bytes = ready!(this.future.poll(cx))?;
        Poll::Ready(serde_json::from_slice(&bytes).map_err(Error::other))
    }
}

pin_project! {
    pub struct TextFuture {
        #[pin]
        future: BytesFuture,
    }
}

impl Future for TextFuture {
    type Output = Result<String, Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let bytes = ready!(this.future.poll(cx))?;
        Poll::Ready(String::from_utf8(bytes.to_vec()).map_err(Error::other))
    }
}

/// Response shim to work around [an issue in reqwest](https://github.com/seanmonstar/reqwest/issues/1234) (there is an [open pull request](https://github.com/seanmonstar/reqwest/pull/1532) fixing this).
///
/// Reqwest doesn't limit the response body size by default nor does it offer an option to configure one.
/// Since we have to fetch data from untrusted sources, not restricting the maximum size is a DoS hazard for us.
///
/// This shim reimplements the `bytes`, `json`, and `text` functions and restricts the bodies to 100KB.
///
/// TODO: Remove this shim as soon as reqwest gets support for size-limited bodies.
pub trait ResponseExt {
    type BytesFuture;
    type JsonFuture<T>;
    type TextFuture;

    /// Size limited version of `bytes` to work around a reqwest issue. Check [`ResponseExt`] docs for details.
    fn bytes_limited(self) -> Self::BytesFuture;
    /// Size limited version of `json` to work around a reqwest issue. Check [`ResponseExt`] docs for details.
    fn json_limited<T>(self) -> Self::JsonFuture<T>;
    /// Size limited version of `text` to work around a reqwest issue. Check [`ResponseExt`] docs for details.
    fn text_limited(self) -> Self::TextFuture;
}

impl ResponseExt for Response {
    type BytesFuture = BytesFuture;
    type JsonFuture<T> = JsonFuture<T>;
    type TextFuture = TextFuture;

    fn bytes_limited(self) -> Self::BytesFuture {
        BytesFuture {
            stream: Box::pin(self.bytes_stream()),
            limit: MAX_BODY_SIZE,
            aggregator: BytesMut::new(),
        }
    }

    fn json_limited<T>(self) -> Self::JsonFuture<T> {
        JsonFuture {
            _t: PhantomData,
            future: self.bytes_limited(),
        }
    }

    fn text_limited(self) -> Self::TextFuture {
        TextFuture {
            future: self.bytes_limited(),
        }
    }
}
