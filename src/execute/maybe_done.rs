use futures::{Async, Future, Poll};
use std::mem;

#[derive(Debug)]
pub(crate) enum MaybeDone<F: Future> {
    NotYet(F),
    Done(F::Item),
    Gone,
}

impl<F: Future> MaybeDone<F> {
    pub(crate) fn new(f: F) -> Self {
        MaybeDone::NotYet(f)
    }

    pub(crate) fn poll_ready(&mut self) -> Poll<(), F::Error> {
        let res = match *self {
            MaybeDone::NotYet(ref mut f) => f.poll()?,
            MaybeDone::Done(_) => return Ok(Async::Ready(())),
            MaybeDone::Gone => panic!("cannot poll Join twice"),
        };
        match res {
            Async::Ready(res) => {
                *self = MaybeDone::Done(res);
                Ok(Async::Ready(()))
            }
            Async::NotReady => Ok(Async::NotReady),
        }
    }

    pub(crate) fn take_ok(&mut self) -> Option<F::Item> {
        match mem::replace(self, MaybeDone::Gone) {
            MaybeDone::Done(ok) => Some(ok),
            _ => None,
        }
    }
}
