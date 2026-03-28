use std::io::Read;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::AsyncRead;

pub struct ProgressReader<R> {
    inner: R,
    pub total: u64,
    pub read: u64,
    progress_callback: Box<dyn Fn(u64, u64, u64)>,
}

impl<R: Read> ProgressReader<R> {
    pub fn new(inner: R, total: u64, callback: impl Fn(u64, u64, u64) + 'static) -> Self {
        Self {
            inner,
            total,
            read: 0,
            progress_callback: Box::new(callback),
        }
    }
}

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        let delta = n as u64;
        self.read += delta;
        (self.progress_callback)(self.read, self.total, delta);
        Ok(n)
    }
}

pub struct AsyncProgressReader<R> {
    inner: R,
    pub total: u64,
    pub read: u64,
    progress_callback: Box<dyn Fn(u64, u64, u64) + Send + Sync>,
}

impl<R: AsyncRead + Unpin> AsyncProgressReader<R> {
    pub fn new(
        inner: R,
        total: u64,
        callback: impl Fn(u64, u64, u64) + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner,
            total,
            read: 0,
            progress_callback: Box::new(callback),
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for AsyncProgressReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let inner = Pin::new(&mut self.inner);
        match inner.poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                let after = buf.filled().len();
                let delta = (after - before) as u64;
                self.read += delta;
                if self.total > 0 {
                    (self.progress_callback)(self.read, self.total, delta);
                }
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}
