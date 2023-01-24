use bytes::{BufMut, Bytes, BytesMut};
use futures_core::{ready, stream::BoxStream, Stream};
use pin_project_lite::pin_project;
use reqwest::Response;
use serde::Deserialize;
use std::{
    future::Future,
    marker::PhantomData,
    mem,
    pin::Pin,
    task::{Context, Poll},
};

use crate::Error;

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
                .map_err(Error::conv)?
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
    T: for<'de> Deserialize<'de>,
{
    type Output = Result<T, Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let bytes = ready!(this.future.poll(cx))?;
        Poll::Ready(serde_json::from_slice(&bytes).map_err(Error::conv))
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
        Poll::Ready(String::from_utf8(bytes.to_vec()).map_err(Error::conv))
    }
}

pub trait ResponseExt {
    type BytesFuture;
    type JsonFuture<T>;
    type TextFuture;

    fn bytes_limited(self, limit: usize) -> Self::BytesFuture;
    fn json_limited<T>(self, limit: usize) -> Self::JsonFuture<T>;
    fn text_limited(self, limit: usize) -> Self::TextFuture;
}

impl ResponseExt for Response {
    type BytesFuture = BytesFuture;
    type JsonFuture<T> = JsonFuture<T>;
    type TextFuture = TextFuture;

    fn bytes_limited(self, limit: usize) -> Self::BytesFuture {
        BytesFuture {
            stream: Box::pin(self.bytes_stream()),
            limit,
            aggregator: BytesMut::new(),
        }
    }

    fn json_limited<T>(self, limit: usize) -> Self::JsonFuture<T> {
        JsonFuture {
            _t: PhantomData,
            future: self.bytes_limited(limit),
        }
    }

    fn text_limited(self, limit: usize) -> Self::TextFuture {
        TextFuture {
            future: self.bytes_limited(limit),
        }
    }
}
