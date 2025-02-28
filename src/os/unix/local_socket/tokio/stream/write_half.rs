use {
    crate::os::unix::udsocket::tokio::WriteHalf as WriteHalfImpl,
    futures_io::AsyncWrite,
    std::{
        fmt::{self, Debug, Formatter},
        io::{self, IoSlice},
        pin::Pin,
        task::{Context, Poll},
    },
};

pub struct WriteHalf(pub(super) WriteHalfImpl);
impl WriteHalf {
    #[inline]
    fn pinproj(&mut self) -> Pin<&mut WriteHalfImpl> {
        Pin::new(&mut self.0)
    }
}
impl AsyncWrite for WriteHalf {
    #[inline]
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        self.pinproj().poll_write(cx, buf)
    }
    #[inline]
    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        self.pinproj().poll_write_vectored(cx, bufs)
    }
    #[inline]
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.pinproj().poll_flush(cx)
    }
    #[inline]
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.pinproj().poll_close(cx)
    }
}
impl Debug for WriteHalf {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("local_socket::WriteHalf").field(&self.0).finish()
    }
}
forward_as_handle!(WriteHalf);
