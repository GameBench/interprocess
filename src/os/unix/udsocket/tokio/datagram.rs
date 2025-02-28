use crate::os::unix::udsocket::{ToUdSocketPath, UdDatagram as SyncUdDatagram, UdSocketPath};
use std::{
    future::Future,
    io,
    os::unix::net::UnixDatagram as StdUdDatagram,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::{io::ReadBuf as TokioReadBuf, net::UnixDatagram as TokioUdDatagram};

/// A Unix domain datagram socket, obtained either from [`UdSocketListener`](super::UdSocketListener) or by connecting
/// to an existing server.
///
/// # Examples
///
/// ## Basic packet exchange
/// ```no_run
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use interprocess::os::unix::udsocket::tokio::*;
/// use std::{io, mem::MaybeUninit};
/// use tokio::{io::ReadBuf, try_join};
///
/// // Socket creation happens immediately, no futures here.
/// let socket = UdDatagram::bound("/tmp/example_side_a.sock")?;
///
/// // This is the part where you tell the other side
/// // that you've spun up a socket, if you need to.
///
/// // So does destination assignment.
/// socket.set_destination("/tmp/example/side_b.sock")?;
///
/// // Allocate a stack buffer for reading at a later moment.
/// let mut buffer = [MaybeUninit::<u8>::uninit(); 128];
/// let mut readbuf = ReadBuf::uninit(&mut buffer);
///
/// // Describe the send operation, but don't run it yet.
/// // We'll launch it concurrently with the read operation.
/// let send = socket.send(b"Hello from side A!");
///
/// // Describe the receive operation, and also don't run it yet.
/// let recv = socket.recv(&mut readbuf);
///
/// // Perform both operations concurrently: the send and the receive.
/// try_join!(send, recv)?;
///
/// // Clean up early. Good riddance!
/// drop(socket);
///
/// // Convert the data that's been read into a string. This checks for UTF-8
/// // validity, and if invalid characters are found, a new buffer is
/// // allocated to house a modified version of the received data, where
/// // decoding errors are replaced with those diamond-shaped question mark
/// // U+FFFD REPLACEMENT CHARACTER thingies: �.
/// let received_string = String::from_utf8_lossy(readbuf.filled());
///
/// println!("Other side answered: {}", &received_string);
/// # Ok(()) }
/// ```
// TODO update..?
#[derive(Debug)]
pub struct UdDatagram(TokioUdDatagram);
impl UdDatagram {
    /// Creates an unnamed datagram socket.
    pub fn unbound() -> io::Result<Self> {
        let socket = TokioUdDatagram::unbound()?;
        Ok(Self(socket))
    }
    /// Creates a named datagram socket assigned to the specified path. This will be the "home" of this socket. Then,
    /// packets from somewhere else directed to this socket with [`.send_to()`](Self::send_to) or
    /// [`.connect()`](Self::connect) will go here.
    ///
    /// See [`ToUdSocketPath`] for an example of using various string types to specify socket paths.
    pub fn bound<'a>(path: impl ToUdSocketPath<'a>) -> io::Result<Self> {
        Self::_bound(path.to_socket_path()?)
    }
    fn _bound(path: UdSocketPath<'_>) -> io::Result<Self> {
        let socket = TokioUdDatagram::bind(path.as_osstr())?;
        Ok(Self(socket))
    }
    /// Selects the Unix domain socket to send packets to. You can also just use [`.send_to()`](Self::send_to) instead,
    /// but supplying the address to the kernel once is more efficient.
    ///
    /// See [`ToUdSocketPath`] for an example of using various string types to specify socket paths.
    pub fn set_destination<'a>(&self, path: impl ToUdSocketPath<'a>) -> io::Result<()> {
        self._set_destination(path.to_socket_path()?)
    }
    fn _set_destination(&self, path: UdSocketPath<'_>) -> io::Result<()> {
        self.0.connect(path.as_osstr())
    }
    /// Receives a single datagram from the socket, advancing the `ReadBuf` cursor by the datagram length.
    ///
    /// Uses Tokio's [`ReadBuf`](TokioReadBuf) interface. See `.recv_stdbuf()` for a `&mut [u8]` version.
    pub async fn recv(&self, buf: &mut TokioReadBuf<'_>) -> io::Result<()> {
        // Tokio's .recv() uses &mut [u8] instead of &mut TokioReadBuf<'_> for some
        // reason, this works around that
        struct WrapperFuture<'a, 'b, 'c>(&'a UdDatagram, &'b mut TokioReadBuf<'c>);
        impl Future for WrapperFuture<'_, '_, '_> {
            type Output = io::Result<()>;
            fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                self.0 .0.poll_recv(cx, self.1)
            }
        }
        WrapperFuture(self, buf).await
    }
    /// Receives a single datagram from the socket, returning the amount of bytes received.
    ///
    /// Uses an `std`-like `&mut [u8]` interface. See `.recv()` for a version which uses Tokio's
    /// [`ReadBuf`](TokioReadBuf) instead.
    pub async fn recv_stdbuf(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.recv(buf).await
    }
    /// Asynchronously waits until readable data arrives to the socket.
    ///
    /// May finish spuriously – *do not* perform a blocking read when this future finishes and *do* handle a
    /// [`WouldBlock`](io::ErrorKind::WouldBlock) or [`Poll::Pending`].
    pub async fn recv_ready(&self) -> io::Result<()> {
        self.0.readable().await
    }
    /// Sends a single datagram into the socket, returning how many bytes were actually sent.
    pub async fn send(&self, buf: &[u8]) -> io::Result<usize> {
        self.0.send(buf).await
    }
    /// Sends a single datagram to the given address, returning how many bytes were actually sent.
    pub async fn send_to(&self, buf: &[u8], path: impl ToUdSocketPath<'_>) -> io::Result<usize> {
        let path = path.to_socket_path()?;
        self._send_to(buf, &path).await
    }
    async fn _send_to(&self, buf: &[u8], path: &UdSocketPath<'_>) -> io::Result<usize> {
        self.0.send_to(buf, path.as_osstr()).await
    }
    /// Asynchronously waits until the socket becomes writable due to the other side freeing up space in its OS receive
    /// buffer.
    ///
    /// May finish spuriously – *do not* perform a blocking write when this future finishes and *do* handle a
    /// [`WouldBlock`](io::ErrorKind::WouldBlock) or [`Poll::Pending`].
    pub async fn send_ready(&self) -> io::Result<()> {
        self.0.writable().await
    }
    /// Raw polling interface for receiving datagrams. You probably want `.recv()` instead.
    pub fn poll_recv(&self, cx: &mut Context<'_>, buf: &mut TokioReadBuf<'_>) -> Poll<io::Result<()>> {
        self.0.poll_recv(cx, buf)
    }
    /// Raw polling interface for receiving datagrams with an `std`-like receive buffer. You probably want
    /// `.recv_stdbuf()` instead.
    pub fn poll_recv_stdbuf(&self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<()>> {
        let mut readbuf = TokioReadBuf::new(buf);
        self.0.poll_recv(cx, &mut readbuf)
    }
    /// Raw polling interface for sending datagrams. You probably want `.send()` instead.
    pub fn poll_send(&self, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        self.0.poll_send(cx, buf)
    }
    /// Raw polling interface for sending datagrams. You probably want `.send_to()` instead.
    pub fn poll_send_to<'a>(
        &self,
        cx: &mut Context<'_>,
        buf: &[u8],
        path: impl ToUdSocketPath<'a>,
    ) -> Poll<io::Result<usize>> {
        let path = path.to_socket_path()?;
        self._poll_send_to(cx, buf, &path)
    }
    fn _poll_send_to(&self, cx: &mut Context<'_>, buf: &[u8], path: &UdSocketPath<'_>) -> Poll<io::Result<usize>> {
        self.0.poll_send_to(cx, buf, path.as_osstr())
    }
}

tokio_wrapper_trait_impls!(
    for UdDatagram,
    sync SyncUdDatagram,
    std StdUdDatagram,
    tokio TokioUdDatagram);
derive_asraw!(unix: UdDatagram);
