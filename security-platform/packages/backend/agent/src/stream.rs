//! SSE/WebSocket event stream for agent events.

use tokio::sync::mpsc;
use futures_util::stream::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct EventStream<T> {
    rx: mpsc::UnboundedReceiver<Result<T, anyhow::Error>>,
}

impl<T> EventStream<T> {
    pub fn new(rx: mpsc::UnboundedReceiver<Result<T, anyhow::Error>>) -> Self {
        Self { rx }
    }
}

impl<T> Stream for EventStream<T> {
    type Item = Result<T, anyhow::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}