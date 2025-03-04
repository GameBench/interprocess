//! Socket control message manipulation.
//!
//! This module contains a generic safe framework for control messages – not just for Unix domain sockets, but for any
//! `recvmsg`/`sendmsg`-based API in the Unix socket architecture. **The terms "control message" and "ancillary data"
//! are used largely interchangeably**, but the latter is preferred when the "uncountable" nature of heaps of bytes
//! which can be deserialized into control messages needs to be emphasized.
//!
//! The [`ancillary`] module contains safe wrappers that can help you correctly initialize and parse ancillary data
//! control messages; its types can then be fed into any type that implements [`CmsgMut`] via the `.add_message()`
//! method.
//!
//! # Ancillary data validity
//! *Note:* this section pertains only to direct manipulations with raw ancillary data and wrapperless control messages.
//! Most users should simply avoid the use of unsafe code by using the [`ancillary`] module – if "`unsafe`" is never
//! uttered, understanding of this section is superfluous and unnecessary. Additionally, some unsafe functions in the
//! `ancillary` module do not require their user to worry about ancillary data validity. Please do make sure to consult
//! their safety notes to ensure that you're being aware of all necessary contracts and safety phenomena.
//!
//! This module uses a concept of validity of control messages. For simplicity of unsafe code, it is completely and
//! entirely conflated with [`MaybeUninit`]'s concept of well-initialized data, which makes it an extension of that
//! validity property.
//!
//! Two layers of validity contracts are imposed: one at the ancillary data buffer level and one at the [`Cmsg`] level.
//! The former is also an extension of the latter, and both of them are an extension of Rust's well-initialized data
//! contract.
//!
//! ## `Cmsg` validity
//! The `data()` field of the `Cmsg` struct is required to adhere to the requirement that its contents are "valid
//! control messages". What that actually means depends on the specific type of control message as specified by the
//! `cmsg_level` and `cmsg_type` fields. This gives rise to the first common requirement: the data inside `data` must
//! match the claimed level and type.
//!
//! The specifics of other constrains are given by the system manpages; the most common kind of constraint specified
//! there is the size constraint on control messages represented as fixed-size structs.
//!
//! ## Ancillary data buffer validity
//! This contract, which binds the well-initialized portion of [`CmsgRef`] and any type which implements [`CmsgMut`], is
//! trivially derived from `Cmsg` validity via the following set of requirements:
//! - The data must be well-initialized in the sense described by the documentation of [`MaybeUninit`].
//! - When parsed into `Cmsg`s, the control messages must uphold `Cmsg` validity.
//!
//! [`MaybeUninit`]: std::mem::MaybeUninit
// TODO parser

pub mod ancillary;

pub(super) mod cmsg_mut;
mod mref;
mod mut_buf;
mod vec_buf;

pub use {cmsg_mut::*, mref::*, mut_buf::*, vec_buf::*};

use super::util::{to_msghdr_controllen, CmsghdrLen};
use libc::{c_int, c_uint, cmsghdr, msghdr};
use std::{
    ffi::c_void,
    mem::{align_of, zeroed, MaybeUninit},
};

/// A **c**ontrol **m**e**s**sa**g**e, consisting of a level, type and its payload.
///
/// The type encodes the memory safety of the control message with the specified level, type and payload being sent, in
/// the form of a safety guarantee. It intentionally does not implement [`Copy`] and [`Clone`] because control messages,
/// as exemplified by [`ancillary::FileDescriptors`](ancillary::file_descriptors::FileDescriptors), can transfer
/// ownership over resources, which requires that only move semantics be provided.
#[derive(Debug, PartialEq, Eq)]
pub struct Cmsg<'a> {
    cmsg_level: c_int,
    cmsg_type: c_int,
    data: &'a [u8],
}
impl<'a> Cmsg<'a> {
    /// Constructs a control message with the given level, type and payload.
    ///
    /// # Safety
    /// - The length of `data` must not exceed the maximum value of `c_uint`.
    /// - The contents of `data` are not checked with respect to the supplied `cmsg_level` and cmsg_type`, which means
    /// that OS-specific functionality invoked via ancillary data cannot be accounted for or validated. For example,
    /// passing file descriptors in this way can invalidate Rust RAII ownership of them as resources, effectively
    /// violating the contract of `FromRawFd` via the use of `AsRawFd`.
    #[inline(always)]
    pub const unsafe fn new(cmsg_level: c_int, cmsg_type: c_int, data: &'a [u8]) -> Self {
        assert!(
            data.len() <= c_uint::MAX as usize,
            "length of payload does not fit in c_uint"
        );
        Self {
            cmsg_level,
            cmsg_type,
            data,
        }
    }
    /// Returns the `cmsg_len` of a control message with a payload of the given size.
    ///
    /// The type of the return value is platform-independent, but values will never overflow the actual type used in
    /// `cmsghdr` to store `cmsg_len`. The function simply panics if an offending size is encountered.
    ///
    /// # Panics
    /// If the computed size exceeds the maximum for the `cmsg_len` field on `cmsghdr`.
    pub const fn cmsg_len_for_payload_size(payload_size: c_uint) -> usize {
        // FIXME potential portability concern, Linux says that it's only planned for inclusion into POSIX
        let len = unsafe { libc::CMSG_LEN(payload_size) };
        if len > CmsghdrLen::MAX as _ {
            panic!("cmsg_len overflowed the storage type in cmsghdr");
        }
        len as usize
    }
    /// Returns the `cmsg_len` of the control message – an alias for
    /// `Self::cmsg_len_for_payload_size(self.data.len())`.
    ///
    /// The type of the return value is platform-independent, but values will never overflow the actual type used in
    /// `cmsghdr` to store `cmsg_len`. The function simply panics if an offending size is encountered.
    ///
    /// # Panics
    /// If the computed size exceeds the maximum for the `cmsg_len` field on `cmsghdr`.
    #[inline(always)]
    pub const fn cmsg_len(&self) -> usize {
        Self::cmsg_len_for_payload_size(self.data.len() as c_uint)
    }
    /// Returns the `cmsg_level` of the control message.
    #[inline(always)]
    pub const fn cmsg_level(&self) -> c_int {
        self.cmsg_level
    }
    /// Returns the `cmsg_type` of the control message.
    #[inline(always)]
    pub const fn cmsg_type(&self) -> c_int {
        self.cmsg_type
    }
    /// Returns the payload of the control message.
    #[inline(always)]
    pub const fn data(&self) -> &'a [u8] {
        self.data
    }
    /// Returns the amount of space the control message occupies in a control message buffer, including its `cmsghdr`
    /// and all necessary padding.
    #[inline(always)]
    pub const fn space_occupied(&self) -> usize {
        unsafe { libc::CMSG_SPACE(self.data.len() as c_uint) as usize }
    }
    /// Clones the control message. No special treatment of the contained data is performed, and the struct is simply
    /// copied bitwise, with the data slice pointing to the same memory.
    ///
    /// # Safety
    /// As outlined in the [struct-level documentation](Cmsg), control messages can potentially and unknowingly have
    /// ownership over resources (such as [file descriptors](ancillary::file_descriptors::FileDescriptors)), which means
    /// that cloning the raw control message and then parsing it twice can lead to a double-free scenario. This method
    /// should only be called if the original copy then never gets parsed, is known to not own any resources as per
    /// `cmsg_level` and `cmsg_type` or if the potential unsafe double-free outcome is averted by some other means.
    #[inline(always)]
    pub const unsafe fn clone_unchecked(&self) -> Self {
        Self {
            cmsg_level: self.cmsg_level,
            cmsg_type: self.cmsg_type,
            data: self.data,
        }
    }
}

fn dummy_msghdr(buf: &[MaybeUninit<u8>]) -> msghdr {
    let mut hdr = unsafe { zeroed::<msghdr>() };
    hdr.msg_control = buf.as_ptr().cast::<c_void>().cast_mut();
    hdr.msg_controllen = to_msghdr_controllen(buf.len()).expect("failed to create dummy msghdr");
    hdr
}

/// Computes an index to the first byte in the buffer in which a `cmsghdr` would be well-aligned.
///
/// The returned location is guaranteed to be able to fit a `cmsghdr`.
fn align_first(buf: &[MaybeUninit<u8>]) -> Option<usize> {
    // The potentially misaligned address
    let base = buf.as_ptr() as usize;
    let aligned = align_up(base, align_of::<cmsghdr>());
    // The amount by which the start must be moved forward to become aligned
    let fwd_align = aligned - base;

    let mut hdr = dummy_msghdr(buf);
    hdr.msg_control = hdr.msg_control.wrapping_add(fwd_align);
    // The cast here is fine because no one in their right mind expects the alignment adjustment to
    // exceed the sort of integer type used for controllen
    hdr.msg_controllen = hdr.msg_controllen.saturating_sub(fwd_align as _);

    let base = unsafe { libc::CMSG_FIRSTHDR(&hdr) }.cast::<MaybeUninit<u8>>();
    if base.is_null() {
        return None;
    }

    let base_idx = unsafe {
        // SAFETY: CMSG_FIRSTHDR never returns a pointer outside the buffer if the return value is non-null
        base.offset_from(buf.as_ptr())
    };
    debug_assert!(base_idx >= 0);
    Some(base_idx as usize)
}

fn align_up(base: usize, align: usize) -> usize {
    let mask = align - 1;
    // Adding the mask pushes any misaligned address over the edge, but puts a well-aligned one
    // just a single byte short of going forward by the alignment's worth of offset
    let nudged = base + mask;
    // Masking the misalignment out puts us at an aligned location
    nudged & !mask
}
