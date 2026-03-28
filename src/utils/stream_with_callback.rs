use futures_util::stream::{Stream, StreamExt};
use pin_project::pin_project;
use std::pin::Pin;
use std::task::{Context, Poll};

#[pin_project]
pub struct StreamWithCallback<St, C> {
    #[pin]
    inner: St,
    callback: Option<C>,
    called: bool,
}

impl<St, C> StreamWithCallback<St, C>
where
    St: Stream,
    C: FnOnce() + Send + Sync + 'static,
{
    pub fn new(inner: St, callback: C) -> Self {
        Self {
            inner,
            callback: Some(callback),
            called: false,
        }
    }
}

impl<St, C> Stream for StreamWithCallback<St, C>
where
    St: Stream,
    C: FnOnce() + Send + Sync + 'static,
{
    type Item = St::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();

        match this.inner.poll_next(cx) {
            Poll::Ready(Some(item)) => Poll::Ready(Some(item)),
            Poll::Ready(None) => {
                if !*this.called {
                    *this.called = true;
                    if let Some(callback) = this.callback.take() {
                        callback();
                    }
                }
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

pub trait StreamCallbackExt: Stream + Sized {
    fn on_complete<C>(self, callback: C) -> StreamWithCallback<Self, C>
    where
        C: FnOnce() + Send + Sync + 'static,
    {
        StreamWithCallback::new(self, callback)
    }
}

impl<St: Stream> StreamCallbackExt for St {}
