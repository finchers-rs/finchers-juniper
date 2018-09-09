use pin_utils::unsafe_pinned;
use std::fmt;
use std::pin::PinMut;

use futures::future;
use futures::future::{Future, TryFuture, TryFutureExt};
use futures::ready;
use futures::task;
use futures::task::Poll;

pub struct MaybeDone<F: TryFuture> {
    inner: future::MaybeDone<future::IntoFuture<F>>,
}

impl<F: TryFuture> fmt::Debug for MaybeDone<F>
where
    F: fmt::Debug,
    F::Ok: fmt::Debug,
    F::Error: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("MaybeDone").field(&self.inner).finish()
    }
}

impl<F: TryFuture> MaybeDone<F> {
    unsafe_pinned!(inner: future::MaybeDone<future::IntoFuture<F>>);

    pub fn new(f: F) -> Self {
        MaybeDone {
            inner: future::maybe_done(f.into_future()),
        }
    }

    pub fn poll_ready(
        mut self: PinMut<'_, Self>,
        cx: &mut task::Context<'_>,
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

    pub fn take_ok(mut self: PinMut<'_, Self>) -> Option<F::Ok> {
        match self.inner().take_output() {
            Some(Ok(v)) => Some(v),
            _ => None,
        }
    }
}
