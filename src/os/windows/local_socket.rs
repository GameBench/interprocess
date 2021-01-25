use super::named_pipe::{
    DuplexBytePipeStream as PipeStream, PipeListener as GenericPipeListener, PipeListenerOptions,
    PipeMode,
};
use crate::local_socket::{LocalSocketName, NameTypeSupport, ToLocalSocketName};
use std::{
    borrow::Cow,
    ffi::{c_void, OsStr, OsString},
    fmt::{self, Debug, Formatter},
    io::{self, prelude::*, IoSlice, IoSliceMut},
    os::windows::io::{AsRawHandle, FromRawHandle, IntoRawHandle},
    ptr,
    sync::atomic::{AtomicU8, Ordering},
};
use winapi::um::{namedpipeapi::GetNamedPipeInfo, winbase::PIPE_SERVER_END};

type PipeListener = GenericPipeListener<PipeStream>;

pub struct LocalSocketListener {
    inner: PipeListener,
}
impl LocalSocketListener {
    #[inline]
    pub fn bind<'a>(name: impl ToLocalSocketName<'a>) -> io::Result<Self> {
        let name = name.to_local_socket_name()?;
        let inner = PipeListenerOptions::new()
            .name(name.into_inner())
            .mode(PipeMode::Bytes)
            .create()?;
        Ok(Self { inner })
    }
    #[inline]
    pub fn accept(&self) -> io::Result<LocalSocketStream> {
        let inner = self.inner.accept()?;
        Ok(LocalSocketStream {
            inner,
            server_or_client: AtomicU8::new(ServerOrClient::Server as _),
        })
    }
}
impl Debug for LocalSocketListener {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("LocalSocketListener")
    }
}

pub struct LocalSocketStream {
    inner: PipeStream,
    server_or_client: AtomicU8,
}
#[repr(u8)]
enum ServerOrClient {
    Client = 0,
    Server = 1,
    Nah = 2,
}
impl From<u8> for ServerOrClient {
    #[inline]
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Client,
            1 => Self::Server,
            _ => Self::Nah,
        }
    }
}
impl LocalSocketStream {
    pub fn connect<'a>(name: impl ToLocalSocketName<'a>) -> io::Result<Self> {
        let name = name.to_local_socket_name()?;
        let inner = PipeStream::connect(name.inner())?;
        Ok(Self {
            inner,
            server_or_client: AtomicU8::new(ServerOrClient::Client as _),
        })
    }
    #[inline]
    pub fn peer_pid(&self) -> io::Result<u32> {
        match self.server_or_client.load(Ordering::Relaxed).into() {
            ServerOrClient::Server => self.inner.client_process_id(),
            ServerOrClient::Client => self.inner.server_process_id(),
            ServerOrClient::Nah => {
                let mut flags: u32 = 0;
                let success = unsafe {
                    GetNamedPipeInfo(
                        self.as_raw_handle(),
                        &mut flags as *mut _,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    )
                } != 0;
                if !success {
                    return Err(io::Error::last_os_error());
                }
                // The PIPE_SERVER_END bit is either set or unset and that
                // indicates whether it's a server or client, as opposed to
                // having two different flags in different bits.
                flags &= PIPE_SERVER_END;
                // Round-trip into ServerOrClient to validate and fall back to the Nah variant.
                self.server_or_client
                    .store(ServerOrClient::from(flags as u8) as _, Ordering::Relaxed);
                self.peer_pid()
            }
        }
    }
}
impl Read for LocalSocketStream {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
    #[inline]
    fn read_vectored(&mut self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        self.inner.read_vectored(bufs)
    }
}
impl Write for LocalSocketStream {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }
    #[inline]
    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        self.inner.write_vectored(bufs)
    }
    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
impl Debug for LocalSocketStream {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalSocketStream")
            .field("handle", &self.as_raw_handle())
            .finish()
    }
}
impl AsRawHandle for LocalSocketStream {
    #[inline]
    fn as_raw_handle(&self) -> *mut c_void {
        self.inner.as_raw_handle()
    }
}
impl IntoRawHandle for LocalSocketStream {
    #[inline]
    fn into_raw_handle(self) -> *mut c_void {
        self.inner.into_raw_handle()
    }
}
impl FromRawHandle for LocalSocketStream {
    #[inline]
    unsafe fn from_raw_handle(handle: *mut c_void) -> Self {
        Self {
            inner: PipeStream::from_raw_handle(handle),
            server_or_client: AtomicU8::new(ServerOrClient::Nah as _),
        }
    }
}

pub const NAME_TYPE_ALWAYS_SUPPORTED: NameTypeSupport = NameTypeSupport::OnlyNamespaced;

#[inline]
pub fn name_type_support_query() -> NameTypeSupport {
    NAME_TYPE_ALWAYS_SUPPORTED
}
#[inline]
pub fn to_local_socket_name_osstr(osstr: &OsStr) -> LocalSocketName<'_> {
    LocalSocketName::from_raw_parts(Cow::Borrowed(osstr), true)
}
#[inline]
pub fn to_local_socket_name_osstring(osstring: OsString) -> LocalSocketName<'static> {
    LocalSocketName::from_raw_parts(Cow::Owned(osstring), true)
}

/*
/// Helper function to check whether a series of UTF-16 bytes starts with `\\.\pipe\`.
fn has_pipefs_prefix(
    val: impl IntoIterator<Item = u16>,
) -> bool {
    let pipefs_prefix: [u16; 9] = [
        // The string \\.\pipe\ in UTF-16
        0x005c, 0x005c, 0x002e, 0x005c, 0x0070, 0x0069, 0x0070, 0x0065, 0x005c,
    ];
    pipefs_prefix.iter().copied().eq(val)

}*/

// TODO add Path/PathBuf special-case for \\.\pipe\*
