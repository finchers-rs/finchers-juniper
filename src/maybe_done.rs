use std::pin::PinMut;

use futures::future;
use futures::future::{Future, TryFuture, TryFutureExt};
use futures::ready;
use futures::task;
use futures::task::Poll;
use pin_utils::unsafe_pinned;

pub struct MaybeDone<F: TryFuture> {
    inner: future::MaybeDone<future::IntoFuture<F>>,
}

impl<F: TryFuture> MaybeDone<F> {
    unsafe_pinned!(inner: future::MaybeDone<future::IntoFuture<F>>);

    pub fn new(f: F) -> Self {
        MaybeDone {
            inner: future::maybe_done(f.into_future()),
        }
    }

    pub fn poll_ready(
        mut self: PinMut<Self>,
        cx: &mut task::Context,
    ) -> Poll<Result<(), F::Error>> {
        ready!(self.inner().poll(cx));
        match self.inner().output_mut() {
            Some(Err(..)) => Poll::Ready(Err(self
                .inner()
                .take_output()
                .expect("should be a Some(t)")
                .err()
                .expect("should be an Err(err)"))),
            _ => Poll::Ready(Ok(())),
        }
    }

    pub fn take_ok(mut self: PinMut<Self>) -> Option<F::Ok> {
        match self.inner().take_output() {
            Some(Ok(v)) => Some(v),
            _ => None,
        }
    }
}
