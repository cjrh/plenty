//! Sockets and name resolution for `std.net`. Each call is one request to the
//! kernel or, for resolution, one `getaddrinfo`. Listening, datagram and
//! connecting sockets are non-blocking, so `std.net` bounds every wait with
//! `std.ready`; a connected or accepted stream blocks, and `std.net` waits for
//! readiness before each transfer. Every descriptor is close-on-exec and is
//! owned by a `File`, so dropping the owner closes it.
//!
//! An address crosses the ABI as bytes: the family (4 or 6), the port in
//! network order, the address and, for IPv6, the scope id in network order.
//! `getaddrinfo` allocates its result list with the C allocator, outside
//! Plenty's accounting, and frees it before returning.
use super::filesystem::{byte_list, contiguous};
use super::Type;
use crate::text_io::{self, Error, Kind};

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
mod native {
    use super::super::byte_storage::{self, Storage};
    use super::*;
    use crate::memory::AllocError;
    use std::os::fd::{FromRawFd, OwnedFd};

    const AF_UNSPEC: i32 = 0;
    const AF_INET: i32 = 2;
    const AF_INET6: i32 = 10;
    const SOCK_STREAM: i32 = 1;
    const SOCK_DGRAM: i32 = 2;
    const SOCK_NONBLOCK: i32 = 0o4000;
    const SOCK_CLOEXEC: i32 = 0o2000000;
    const SOL_SOCKET: i32 = 1;
    const SO_REUSEADDR: i32 = 2;
    const SO_ERROR: i32 = 4;
    const SO_BROADCAST: i32 = 6;
    const IPPROTO_TCP: i32 = 6;
    const TCP_NODELAY: i32 = 1;
    const IPPROTO_IPV6: i32 = 41;
    const IPV6_V6ONLY: i32 = 26;
    const EINPROGRESS: i32 = 115;
    const F_GETFL: i32 = 3;
    const F_SETFL: i32 = 4;
    const O_NONBLOCK: i32 = 0o4000;
    const MSG_TRUNC: i32 = 0x20;
    const MSG_NOSIGNAL: i32 = 0x4000;
    const EAI_NONAME: i32 = -2;
    const EAI_AGAIN: i32 = -3;
    const EAI_NODATA: i32 = -5;
    const EAI_MEMORY: i32 = -10;
    const EAI_SYSTEM: i32 = -11;
    const EAGAIN: i32 = 11;

    const LISTEN: u8 = 1;
    const DATAGRAM: u8 = 2;
    const CONNECT: u8 = 3;

    /// `sockaddr_storage`: 128 bytes, aligned for any address family.
    #[repr(C, align(8))]
    struct SocketAddress([u8; 128]);

    #[repr(C)]
    struct AddrInfo {
        flags: i32,
        family: i32,
        socktype: i32,
        protocol: i32,
        addrlen: u32,
        addr: *const SocketAddress,
        canonname: *mut u8,
        next: *mut AddrInfo,
    }

    extern "C" {
        fn socket(domain: i32, kind: i32, protocol: i32) -> i32;
        fn bind(fd: i32, address: *const SocketAddress, length: u32) -> i32;
        fn listen(fd: i32, backlog: i32) -> i32;
        fn connect(fd: i32, address: *const SocketAddress, length: u32) -> i32;
        fn accept4(fd: i32, address: *mut SocketAddress, length: *mut u32, flags: i32) -> i32;
        fn getsockname(fd: i32, address: *mut SocketAddress, length: *mut u32) -> i32;
        fn getpeername(fd: i32, address: *mut SocketAddress, length: *mut u32) -> i32;
        fn getsockopt(fd: i32, level: i32, name: i32, value: *mut i32, length: *mut u32) -> i32;
        fn setsockopt(fd: i32, level: i32, name: i32, value: *const i32, length: u32) -> i32;
        fn shutdown(fd: i32, how: i32) -> i32;
        fn fcntl(fd: i32, command: i32, ...) -> i32;
        fn sendto(
            fd: i32,
            data: *const u8,
            length: usize,
            flags: i32,
            address: *const SocketAddress,
            address_length: u32,
        ) -> isize;
        fn recvfrom(
            fd: i32,
            data: *mut u8,
            length: usize,
            flags: i32,
            address: *mut SocketAddress,
            address_length: *mut u32,
        ) -> isize;
        fn getaddrinfo(
            node: *const u8,
            service: *const u8,
            hints: *const AddrInfo,
            result: *mut *mut AddrInfo,
        ) -> i32;
        fn freeaddrinfo(result: *mut AddrInfo);
    }

    fn checked(status: i32) -> Result<i32, Error> {
        if status < 0 {
            Err(std::io::Error::last_os_error().into())
        } else {
            Ok(status)
        }
    }

    fn option(fd: i32, level: i32, name: i32, value: i32) -> Result<(), Error> {
        // SAFETY: a live descriptor and a four-byte integer option.
        checked(unsafe { setsockopt(fd, level, name, &value, 4) }).map(|_| ())
    }

    /// The native form of an encoded address, and its length.
    fn native_address(encoded: &[u8]) -> Result<(SocketAddress, u32, i32), Error> {
        let mut out = SocketAddress([0; 128]);
        let family = match (encoded.first(), encoded.len()) {
            (Some(4), 7) => AF_INET,
            (Some(6), 23) => AF_INET6,
            _ => return Err(Kind::InvalidInput.into()),
        };
        out.0[..2].copy_from_slice(&(family as u16).to_ne_bytes());
        out.0[2..4].copy_from_slice(&encoded[1..3]);
        if family == AF_INET {
            out.0[4..8].copy_from_slice(&encoded[3..7]);
            Ok((out, 16, family))
        } else {
            // Flow information (bytes 4..8) stays zero.
            out.0[8..24].copy_from_slice(&encoded[3..19]);
            let scope = u32::from_be_bytes([encoded[19], encoded[20], encoded[21], encoded[22]]);
            out.0[24..28].copy_from_slice(&scope.to_ne_bytes());
            Ok((out, 28, family))
        }
    }

    /// Appends the encoding of a native address; other families are rejected.
    fn encode(address: &SocketAddress, out: &mut Vec<u8>) -> Result<(), Error> {
        let bytes = &address.0;
        let family = u16::from_ne_bytes([bytes[0], bytes[1]]) as i32;
        let size = match family {
            AF_INET => 7,
            AF_INET6 => 23,
            _ => return Err(Kind::Unsupported.into()),
        };
        out.try_reserve(size).map_err(|_| AllocError::OutOfMemory)?;
        if family == AF_INET {
            out.push(4);
            out.extend_from_slice(&bytes[2..4]);
            out.extend_from_slice(&bytes[4..8]);
        } else {
            out.push(6);
            out.extend_from_slice(&bytes[2..4]);
            out.extend_from_slice(&bytes[8..24]);
            let scope = u32::from_ne_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
            out.extend_from_slice(&scope.to_be_bytes());
        }
        Ok(())
    }

    fn adopted(fd: i32, ty: &'static Type) -> Result<u128, Error> {
        // SAFETY: `fd` is a fresh descriptor that nothing else owns.
        let owned = unsafe { OwnedFd::from_raw_fd(fd) };
        crate::files::adopt_socket(std::fs::File::from(owned), ty)
    }

    /// Creates a socket for `kind` and binds, listens or starts connecting.
    /// A connection that is still in progress is success: the caller waits
    /// for the socket to become writable, then calls `finish_connect`.
    fn open(encoded: &[u8], kind: u8, backlog: i64, ty: &'static Type) -> Result<u128, Error> {
        let (address, length, family) = native_address(encoded)?;
        let style = if kind == DATAGRAM {
            SOCK_DGRAM
        } else {
            SOCK_STREAM
        };
        if !matches!(kind, LISTEN | DATAGRAM | CONNECT) {
            return Err(Kind::InvalidInput.into());
        }
        // SAFETY: plain integer arguments.
        let fd = checked(unsafe { socket(family, style | SOCK_NONBLOCK | SOCK_CLOEXEC, 0) })?;
        // SAFETY: the descriptor was just created and is owned here only.
        let owned = unsafe { OwnedFd::from_raw_fd(fd) };
        if family == AF_INET6 && kind != CONNECT {
            // An IPv6 socket serves IPv6 only, whatever the system default.
            option(fd, IPPROTO_IPV6, IPV6_V6ONLY, 1)?;
        }
        match kind {
            LISTEN => {
                option(fd, SOL_SOCKET, SO_REUSEADDR, 1)?;
                // SAFETY: a native address of the stated length.
                checked(unsafe { bind(fd, &address, length) })?;
                let backlog = backlog.clamp(1, 4096) as i32;
                // SAFETY: a bound stream socket.
                checked(unsafe { listen(fd, backlog) })?;
            }
            DATAGRAM => {
                // SAFETY: a native address of the stated length.
                checked(unsafe { bind(fd, &address, length) })?;
            }
            _ => {
                // SAFETY: a native address of the stated length.
                if unsafe { connect(fd, &address, length) } < 0 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() != Some(EINPROGRESS) {
                        return Err(error.into());
                    }
                }
            }
        }
        crate::files::adopt_socket(std::fs::File::from(owned), ty)
    }

    /// The accepted socket blocks; `std.net` waits for readiness first. An
    /// empty queue is the error EAGAIN.
    fn accept(fd: i32, ty: &'static Type) -> Result<u128, Error> {
        // SAFETY: a listening descriptor; the peer address is not requested.
        let accepted = checked(unsafe {
            accept4(fd, std::ptr::null_mut(), std::ptr::null_mut(), SOCK_CLOEXEC)
        })?;
        adopted(accepted, ty)
    }

    /// Reports a failed connection attempt, or makes the connected socket
    /// blocking. A connection still in progress is EAGAIN.
    fn finish_connect(fd: i32) -> Result<u128, Error> {
        let (mut error, mut length) = (0i32, 4u32);
        // SAFETY: a four-byte integer option.
        checked(unsafe { getsockopt(fd, SOL_SOCKET, SO_ERROR, &mut error, &mut length) })?;
        if error != 0 {
            return Err(std::io::Error::from_raw_os_error(error).into());
        }
        let mut peer = SocketAddress([0; 128]);
        let mut length = 128u32;
        // SAFETY: storage for any address family.
        if unsafe { getpeername(fd, &mut peer, &mut length) } < 0 {
            return Err(std::io::Error::from_raw_os_error(EAGAIN).into());
        }
        // SAFETY: integer commands on a live descriptor.
        let flags = checked(unsafe { fcntl(fd, F_GETFL) })?;
        checked(unsafe { fcntl(fd, F_SETFL, flags & !O_NONBLOCK) })?;
        Ok(0)
    }

    fn address(fd: i32, peer: bool, ty: &'static Type) -> Result<u128, Error> {
        let mut native = SocketAddress([0; 128]);
        let mut length = 128u32;
        // SAFETY: storage for any address family.
        checked(unsafe {
            if peer {
                getpeername(fd, &mut native, &mut length)
            } else {
                getsockname(fd, &mut native, &mut length)
            }
        })?;
        let mut out = Vec::new();
        encode(&native, &mut out)?;
        // SAFETY: `ty` describes list[u8].
        unsafe { byte_list(&out, ty) }
    }

    fn set_option(fd: i32, which: u128, value: u128) -> Result<u128, Error> {
        let value = (value != 0) as i32;
        match which {
            1 => option(fd, IPPROTO_TCP, TCP_NODELAY, value)?,
            2 => option(fd, SOL_SOCKET, SO_BROADCAST, value)?,
            _ => return Err(Kind::InvalidInput.into()),
        }
        Ok(0)
    }

    fn send_to(fd: i32, source: &[u8], encoded: &[u8]) -> Result<u128, Error> {
        let (address, length, _) = native_address(encoded)?;
        // SAFETY: a live slice and a native address of the stated length.
        let sent = unsafe {
            sendto(
                fd,
                source.as_ptr(),
                source.len(),
                MSG_NOSIGNAL,
                &address,
                length,
            )
        };
        if sent < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(sent as u128)
    }

    /// Returns the datagram's full length, which exceeds the target when the
    /// datagram was truncated, and replaces `from` with the sender's address.
    fn receive_from(fd: i32, target: &mut [u8], from: &mut Vec<u8>) -> Result<u128, Error> {
        let mut native = SocketAddress([0; 128]);
        let mut length = 128u32;
        // A full address fits without growing `from` after the datagram is gone.
        from.try_reserve(23).map_err(|_| AllocError::OutOfMemory)?;
        // SAFETY: a live slice and storage for any address family.
        let received = unsafe {
            recvfrom(
                fd,
                target.as_mut_ptr(),
                target.len(),
                MSG_TRUNC,
                &mut native,
                &mut length,
            )
        };
        if received < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        from.clear();
        encode(&native, from)?;
        Ok(received as u128)
    }

    fn resolve(host: &str, ty: &'static Type) -> Result<u128, Error> {
        if host.is_empty() || host.contains('\0') {
            return Err(Kind::InvalidInput.into());
        }
        let mut name = Vec::new();
        name.try_reserve(host.len() + 1)
            .map_err(|_| AllocError::OutOfMemory)?;
        name.extend_from_slice(host.as_bytes());
        name.push(0);
        let hints = AddrInfo {
            flags: 0,
            family: AF_UNSPEC,
            socktype: SOCK_STREAM,
            protocol: 0,
            addrlen: 0,
            addr: std::ptr::null(),
            canonname: std::ptr::null_mut(),
            next: std::ptr::null_mut(),
        };
        let mut list = std::ptr::null_mut();
        // SAFETY: a NUL-terminated name, no service and zeroed hints.
        let status = unsafe { getaddrinfo(name.as_ptr(), std::ptr::null(), &hints, &mut list) };
        match status {
            0 => {}
            EAI_NONAME | EAI_NODATA => return unsafe { byte_list(&[], ty) },
            EAI_AGAIN => return Err(std::io::Error::from_raw_os_error(EAGAIN).into()),
            EAI_MEMORY => return Err(AllocError::OutOfMemory.into()),
            EAI_SYSTEM => return Err(std::io::Error::last_os_error().into()),
            _ => return Err(Kind::Other.into()),
        }
        let mut out = Vec::new();
        let mut entry = list;
        let mut outcome = Ok(());
        while !entry.is_null() && outcome.is_ok() {
            // SAFETY: entries of the list getaddrinfo returned, freed below.
            unsafe {
                let address = (*entry).addr;
                if !address.is_null() && matches!((*entry).family, AF_INET | AF_INET6) {
                    outcome = encode(&*address, &mut out);
                }
                entry = (*entry).next;
            }
        }
        // SAFETY: the list getaddrinfo returned, freed exactly once.
        unsafe { freeaddrinfo(list) };
        outcome?;
        // SAFETY: `ty` describes list[u8].
        unsafe { byte_list(&out, ty) }
    }

    pub(super) unsafe fn call(
        op: i64,
        operands: [u128; 5],
        descriptor: &'static Type,
    ) -> Result<u128, Error> {
        let [a, b, c, d, e] = operands;
        let output = descriptor.variants[0].fields[0];
        let file = |value: u128| unsafe { crate::files::descriptor(value as *const _) };
        // SAFETY: compiler signatures check each operand; byte storage and
        // files are live loans for the duration of the call.
        unsafe {
            match op {
                410 => open(&contiguous(a)?, b as u8, c as i64, output),
                411 => accept(file(a)?, output),
                412 => finish_connect(file(a)?),
                413 => address(file(a)?, b == 1, output),
                414 => {
                    let how = match b {
                        0 => 0,
                        1 => 1,
                        2 => 2,
                        _ => return Err(Kind::InvalidInput.into()),
                    };
                    checked(shutdown(file(a)?, how))?;
                    Ok(0)
                }
                415 => set_option(file(a)?, b, c),
                416 => {
                    let bytes = &(*(a as *const Storage)).bytes;
                    let source = &bytes[byte_storage::range(b, c, bytes.len())];
                    send_to(file(d)?, source, &contiguous(e)?)
                }
                417 => {
                    let fd = file(d)?;
                    let bytes = &mut (*(a as *mut Storage)).bytes;
                    let within = byte_storage::range(b, c, bytes.len());
                    let from = &mut (*(e as *mut Storage)).bytes;
                    receive_from(fd, &mut bytes[within], from)
                }
                418 => resolve(
                    crate::strings::utf8(&(a as *const crate::strings::Text)),
                    output,
                ),
                _ => crate::fail("invalid network operation"),
            }
        }
    }
}

/// Operands are live values of the statically checked signature.
pub(super) unsafe fn call(op: i64, operands: [u128; 5], descriptor: *const Type) -> u128 {
    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    {
        // SAFETY: codegen supplies process-lifetime output metadata.
        text_io::result(unsafe { native::call(op, operands, &*descriptor) })
    }
    #[cfg(not(all(target_os = "linux", target_pointer_width = "64")))]
    {
        let _ = (op, operands, descriptor, byte_list, contiguous);
        text_io::result(Err(Kind::Unsupported.into()))
    }
}
