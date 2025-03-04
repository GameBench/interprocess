use super::{
    ancwrap, c_wrappers,
    cmsg::{CmsgMut, CmsgMutBuf, CmsgRef},
    PathDropGuard, ReadAncillarySuccess, ToUdSocketPath, UdSocketPath,
};
use crate::{
    os::unix::{unixprelude::*, FdOps},
    TryClone,
};
#[cfg(target_os = "linux")]
use crate::{
    reliable_recv_msg::{ReliableRecvMsg, TryRecvResult},
    Sealed,
};
use libc::sockaddr_un;
use std::io::{self, prelude::*, IoSlice, IoSliceMut};
use to_method::To;

/// A datagram socket in the Unix domain.
///
/// All such sockets have the `SOCK_DGRAM` socket type; in other words, this is the Unix domain version of a UDP socket.
#[derive(Debug)]
pub struct UdDatagram {
    // TODO make this not 'static
    _drop_guard: PathDropGuard<'static>,
    fd: FdOps,
}
impl UdDatagram {
    /// Creates an unnamed datagram socket.
    ///
    /// # System calls
    /// - `socket`
    pub fn unbound() -> io::Result<Self> {
        let fd = c_wrappers::create_uds(libc::SOCK_DGRAM, false)?;
        Ok(Self {
            _drop_guard: PathDropGuard::dummy(),
            fd,
        })
    }
    /// Binds an existing socket created by [`unbound()`](Self::unbound) to the specified path.
    ///
    /// If the socket path exceeds the [maximum socket path length][mspl] (which includes the first 0 byte when using
    /// the [socket namespace][nmspc]), an error is returned. Errors can also be produced for different reasons, i.e.
    /// errors should always be handled regardless of whether the path is known to be short enough or not.
    ///
    /// After the socket is dropped, the socket file will be left over. Use
    /// [`bound_with_drop_guard()`](Self::bound_with_drop_guard) to mitigate this automatically, even during panics
    /// (if unwinding is enabled).
    ///
    /// # Example
    /// See [`ToUdSocketPath`] for an example of using various string types to specify socket paths.
    ///
    /// # System calls
    /// - `bind`
    ///
    /// [mspl]: super::MAX_UDSOCKET_PATH_LEN
    /// [nmspc]: super::UdSocketPath::Namespaced
    pub fn bind<'a>(&self, path: impl ToUdSocketPath<'a>) -> io::Result<()> {
        self._bind(path.to_socket_path()?)
    }
    fn _bind(&self, path: UdSocketPath<'_>) -> io::Result<()> {
        let addr = path.borrow().try_to::<sockaddr_un>()?;
        unsafe {
            // SAFETY: addr is well-constructed
            c_wrappers::bind(self.as_fd(), &addr)
        }
    }
    /// Binds an existing socket created by [`unbound()`](Self::unbound) to the specified path, remembers the address,
    /// and installs a drop guard that will delete the socket file once the socket is dropped.
    ///
    /// See the documentation of [`bind()`](Self::bind).
    pub fn bind_with_drop_guard<'a>(&mut self, path: impl ToUdSocketPath<'a>) -> io::Result<()> {
        self._bind_with_drop_guard(path.to_socket_path()?)
    }
    fn _bind_with_drop_guard(&mut self, path: UdSocketPath<'_>) -> io::Result<()> {
        self._bind(path.clone())?;
        if matches!(path, UdSocketPath::File(..)) {
            self._drop_guard = PathDropGuard {
                path: path.upgrade(),
                enabled: true,
            };
        }
        Ok(())
    }
    /// Creates a new socket that can be referred to by the specified path.
    ///
    /// If the socket path exceeds the [maximum socket path length][mspl] (which includes the first 0 byte when using
    /// the [socket namespace][nmspc]), an error is returned. Errors can also be produced for different reasons, i.e.
    /// errors should always be handled regardless of whether the path is known to be short enough or not.
    ///
    /// After the socket is dropped, the socket file will be left over. Use
    /// [`bound_with_drop_guard()`](Self::bound_with_drop_guard) to mitigate this automatically, even during panics
    /// (if unwinding is enabled).
    ///
    /// # Example
    /// See [`ToUdSocketPath`] for an example of using various string types to specify socket paths.
    ///
    /// # System calls
    /// - `socket`
    /// - `bind`
    ///
    /// [mspl]: super::MAX_UDSOCKET_PATH_LEN
    /// [nmspc]: super::UdSocketPath::Namespaced
    pub fn bound<'a>(path: impl ToUdSocketPath<'a>) -> io::Result<Self> {
        Self::_bound(path.to_socket_path()?, false)
    }
    /// Creates a new socket that can be referred to by the specified path, remembers the address, and installs a drop
    /// guard that will delete the socket file once the socket is dropped.
    ///
    /// See the documentation of [`bound()`](Self::bound).
    pub fn bound_with_drop_guard<'a>(path: impl ToUdSocketPath<'a>) -> io::Result<Self> {
        Self::_bound(path.to_socket_path()?, true)
    }
    fn _bound(path: UdSocketPath<'_>, keep_drop_guard: bool) -> io::Result<Self> {
        let mut socket = Self::unbound()?;

        if keep_drop_guard {
            socket._bind_with_drop_guard(path)?;
        } else {
            socket._bind(path)?;
        }

        Ok(socket)
    }
    /// Selects the Unix domain socket to send packets to. You can also just use [`.send_to()`](Self::send_to) instead,
    /// but supplying the address to the kernel once is more efficient.
    ///
    /// # Example
    /// ```no_run
    /// use interprocess::os::unix::udsocket::UdDatagram;
    ///
    /// let conn = UdDatagram::bound("/tmp/side_a.sock")?;
    /// conn.set_destination("/tmp/side_b.sock")?;
    /// // Communicate with datagrams here!
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    /// See [`ToUdSocketPath`] for an example of using various string types to specify socket paths.
    ///
    /// # System calls
    /// - `connect`
    pub fn set_destination<'a>(&self, path: impl ToUdSocketPath<'a>) -> io::Result<()> {
        let path = path.to_socket_path()?;
        self._set_destination(&path)
    }
    fn _set_destination(&self, path: &UdSocketPath<'_>) -> io::Result<()> {
        let addr = path.borrow().try_to::<sockaddr_un>()?;
        unsafe {
            // SAFETY: addr is well-constructed
            c_wrappers::connect(self.fd.0.as_fd(), &addr)
        }
    }

    /// Receives a single datagram from the socket, returning the size of the received datagram.
    ///
    /// # System calls
    /// - `read`
    #[inline]
    pub fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        (&self.fd).read(buf)
    }

    /// Receives a single datagram from the socket, making use of [scatter input] and returning the size of the received
    /// datagram.
    ///
    /// # System calls
    /// - `readv`
    ///
    /// [scatter input]: https://en.wikipedia.org/wiki/Vectored_I/O " "
    #[inline]
    pub fn recv_vectored(&self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        (&self.fd).read_vectored(bufs)
    }

    /// Receives a single datagram from the socket along with the control messages attached to it.
    ///
    /// # System calls
    /// - `recvmsg`
    ///
    /// [scatter input]: https://en.wikipedia.org/wiki/Vectored_I/O " "
    #[inline]
    pub fn recv_ancillary(&self, buf: &mut [u8], abuf: &mut impl CmsgMut) -> io::Result<ReadAncillarySuccess> {
        self.recv_ancillary_vectored(&mut [IoSliceMut::new(buf)], abuf)
    }

    /// Receives a single datagram from the socket along with the control messages attached to it, making use of
    /// [scatter input]. The first element of the return value represents the read amount of the former, while the
    /// second element represents that of the latter.
    ///
    /// # System calls
    /// - `recvmsg`
    ///
    /// [scatter input]: https://en.wikipedia.org/wiki/Vectored_I/O " "
    #[inline]
    pub fn recv_ancillary_vectored(
        &self,
        bufs: &mut [IoSliceMut<'_>],
        abuf: &mut impl CmsgMut,
    ) -> io::Result<ReadAncillarySuccess> {
        ancwrap::recvmsg(self.as_fd(), bufs, abuf, None)
    }

    /// Receives a single datagram and the source address from the socket, returning how much of the buffer was filled
    /// out.
    ///
    /// # System calls
    /// - `recvmsg`
    ///     - Future versions of `interprocess` may use `recvfrom` instead; for now, this method is a wrapper around
    /// [`recv_from_vectored`].
    ///
    /// [`recv_from_vectored`]: #method.recv_from_vectored " "
    // TODO use recvfrom
    pub fn recv_from<'a: 'b, 'b>(&self, buf: &mut [u8], addr_buf: &'b mut UdSocketPath<'a>) -> io::Result<usize> {
        self.recv_from_vectored(&mut [IoSliceMut::new(buf)], addr_buf)
    }

    /// Receives a single datagram and the source address from the socket, making use of [scatter input] and returning
    /// how much of the buffer was filled out.
    ///
    /// # System calls
    /// - `recvmsg`
    ///
    /// [scatter input]: https://en.wikipedia.org/wiki/Vectored_I/O " "
    pub fn recv_from_vectored<'a: 'b, 'b>(
        &self,
        bufs: &mut [IoSliceMut<'_>],
        addr_buf: &'b mut UdSocketPath<'a>,
    ) -> io::Result<usize> {
        self.recv_from_ancillary_vectored(bufs, &mut CmsgMutBuf::new(&mut []), addr_buf)
            .map(|x| x.main)
    }

    /// Receives a single datagram, ancillary data and the source address from the socket. The first element of the
    /// return value represents the read amount of the former, while the second element represents that of the latter.
    ///
    /// # System calls
    /// - `recvmsg`
    #[inline]
    pub fn recv_from_ancillary(
        &self,
        buf: &mut [u8],
        abuf: &mut impl CmsgMut,
        addr_buf: &mut UdSocketPath<'_>,
    ) -> io::Result<ReadAncillarySuccess> {
        self.recv_from_ancillary_vectored(&mut [IoSliceMut::new(buf)], abuf, addr_buf)
    }

    /// Receives a single datagram, ancillary data and the source address from the socket, making use of
    /// [scatter input]. The first element of the return value represents the read amount of the former, while the
    /// second element represents that of the latter.
    ///
    /// # System calls
    /// - `recvmsg`
    ///
    /// [scatter input]: https://en.wikipedia.org/wiki/Vectored_I/O " "
    #[inline]
    pub fn recv_from_ancillary_vectored(
        &self,
        bufs: &mut [IoSliceMut<'_>],
        abuf: &mut impl CmsgMut,
        addr_buf: &mut UdSocketPath<'_>,
    ) -> io::Result<ReadAncillarySuccess> {
        ancwrap::recvmsg(self.as_fd(), bufs, abuf, Some(addr_buf))
    }

    /// Returns the size of the next datagram available on the socket without discarding it.
    ///
    /// This method is only available on Linux.2. On other platforms, it's absent and thus any usage of it will result
    /// in a compile-time error.
    ///
    /// # System calls
    /// - `recv`
    #[cfg(target_os = "linux")]
    #[cfg_attr(feature = "doc_cfg", doc(cfg(target_os = "linux")))]
    pub fn peek_msg_size(&self) -> io::Result<usize> {
        let mut buffer = [0_u8; 0];
        let (success, size) = unsafe {
            let size = libc::recv(
                self.as_raw_fd(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                libc::MSG_TRUNC | libc::MSG_PEEK,
            );
            (size != -1, size as usize)
        };
        ok_or_ret_errno!(success => size)
    }

    /// Sends a datagram into the socket.
    ///
    /// # System calls
    /// - `write`
    #[inline]
    pub fn send(&self, buf: &[u8]) -> io::Result<usize> {
        (&self.fd).write(buf)
    }
    // TODO sendto
    /// Sends a datagram into the socket, making use of [gather output] for the main data.
    ///
    ///
    /// # System calls
    /// - `writev`
    ///
    /// [gather output]: https://en.wikipedia.org/wiki/Vectored_I/O " "
    #[inline]
    pub fn send_vectored(&self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        (&self.fd).write_vectored(bufs)
    }
    /// Sends a datagram and ancillary data into the socket.
    ///
    /// # System calls
    /// - `sendmsg`
    #[inline]
    pub fn send_ancillary(&self, buf: &[u8], abuf: CmsgRef<'_>) -> io::Result<usize> {
        self.send_ancillary_vectored(&[IoSlice::new(buf)], abuf)
    }
    /// Sends a datagram and ancillary data into the socket, making use of [gather output] for the main data.
    ///
    /// # System calls
    /// - `sendmsg`
    ///
    /// [gather output]: https://en.wikipedia.org/wiki/Vectored_I/O " "
    #[inline]
    pub fn send_ancillary_vectored(&self, bufs: &[IoSlice<'_>], abuf: CmsgRef<'_>) -> io::Result<usize> {
        ancwrap::sendmsg(self.as_fd(), bufs, abuf)
    }
}

#[cfg(target_os = "linux")]
#[cfg_attr(feature = "doc_cfg", doc(cfg(target_os = "linux")))]
impl ReliableRecvMsg for UdDatagram {
    fn try_recv(&mut self, buf: &mut [u8]) -> io::Result<TryRecvResult> {
        let mut size = self.peek_msg_size()?;
        let fit = size > buf.len();
        if fit {
            size = UdDatagram::recv(self, buf)?;
        }
        Ok(TryRecvResult { size, fit })
    }
}
#[cfg(target_os = "linux")]
impl Sealed for UdDatagram {}

impl TryClone for UdDatagram {
    fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            _drop_guard: self._drop_guard.clone(),
            fd: self.fd.try_clone()?,
        })
    }
}

impl AsFd for UdDatagram {
    #[inline]
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.0.as_fd()
    }
}
impl From<UdDatagram> for OwnedFd {
    #[inline]
    fn from(x: UdDatagram) -> Self {
        x.fd.0
    }
}
impl From<OwnedFd> for UdDatagram {
    fn from(fd: OwnedFd) -> Self {
        UdDatagram {
            _drop_guard: PathDropGuard::dummy(),
            fd: FdOps(fd),
        }
    }
}
derive_raw!(unix: UdDatagram);
