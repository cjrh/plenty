//! Sockets and name resolution for `std.net`. Each call is one request to the
//! kernel or, for resolution, one `getaddrinfo`. All network sockets stay
//! nonblocking, including connected and accepted streams. `std.net` owns
//! readiness waits and retries when readiness becomes stale. Every descriptor
//! is close-on-exec and owned by a `File`, so dropping the owner closes it.
//!
//! An address crosses the ABI as bytes: the family (4 or 6), the port in
//! network order, the address and, for IPv6, the scope id in network order.
//! `getaddrinfo` allocates its result list with the C allocator, outside
//! Plenty's accounting, and frees it before returning.
use super::Type;
use super::filesystem::{byte_list, contiguous};
use crate::text_io::{self, Error, Kind};

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
mod native {
    use super::super::byte_storage::{self, Storage};
    use super::*;
    use crate::memory::AllocError;
    use std::ffi::c_void;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const AF_UNSPEC: i32 = 0;
    const AF_UNIX: i32 = 1;
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
    const TCP_QUICKACK: i32 = 12;
    const IPPROTO_IPV6: i32 = 41;
    const IPV6_V6ONLY: i32 = 26;
    const EINPROGRESS: i32 = 115;
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

    const _: () = {
        assert!(size_of::<SocketAddress>() == size_of::<libc::sockaddr_storage>());
        assert!(align_of::<SocketAddress>() == align_of::<libc::sockaddr_storage>());
        assert!(size_of::<AddrInfo>() == size_of::<libc::addrinfo>());
        assert!(
            std::mem::offset_of!(AddrInfo, addr) == std::mem::offset_of!(libc::addrinfo, ai_addr)
        );
        assert!(
            std::mem::offset_of!(AddrInfo, next) == std::mem::offset_of!(libc::addrinfo, ai_next)
        );
    };

    unsafe extern "C" {
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
        fn socketpair(domain: i32, kind: i32, protocol: i32, ends: *mut i32) -> i32;
        fn send(fd: i32, data: *const u8, length: usize, flags: i32) -> isize;
    }

    type Attributes = libc::pthread_attr_t;
    const PTHREAD_CREATE_DETACHED: i32 = 1;

    #[link(name = "pthread")]
    unsafe extern "C" {
        fn pthread_attr_init(attributes: *mut Attributes) -> i32;
        fn pthread_attr_setstacksize(attributes: *mut Attributes, size: usize) -> i32;
        fn pthread_attr_setdetachstate(attributes: *mut Attributes, state: i32) -> i32;
        fn pthread_attr_destroy(attributes: *mut Attributes) -> i32;
        fn pthread_create(
            thread: *mut usize,
            attributes: *const c_void,
            entry: crate::threads::Entry,
            argument: *mut c_void,
        ) -> i32;
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

    /// Keep accepted streams nonblocking even if readiness becomes stale.
    /// An empty queue is the error EAGAIN.
    fn accept(fd: i32, ty: &'static Type) -> Result<u128, Error> {
        // SAFETY: a listening descriptor; the peer address is not requested.
        let accepted = checked(unsafe {
            accept4(
                fd,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                SOCK_NONBLOCK | SOCK_CLOEXEC,
            )
        })?;
        adopted(accepted, ty)
    }

    /// Reports a failed connection attempt without changing descriptor mode.
    /// A connection still in progress is EAGAIN.
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
            // Leaving delayed-acknowledgement mode sends any pending
            // acknowledgement at once; the kernel re-enters it on its own.
            3 => option(fd, IPPROTO_TCP, TCP_QUICKACK, value)?,
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

    /// Lookups whose helper thread has not finished, including abandoned ones.
    static LOOKUPS: AtomicUsize = AtomicUsize::new(0);
    /// Further lookups are `EAGAIN` (`TryAgain`) until one finishes.
    const MAX_LOOKUPS: usize = 32;
    /// Enough for glibc's resolver and NSS modules; untouched pages cost no
    /// memory.
    const LOOKUP_STACK: usize = 1 << 20;

    // Reply status bytes. A reply is a five-byte header, the status and a
    // big-endian native error code (zero unless `SYSTEM`), then for `FOUND`
    // the encoded addresses.
    const FOUND: u8 = 0;
    const NONE: u8 = 1;
    const AGAIN: u8 = 2;
    const MEMORY: u8 = 3;
    const SYSTEM: u8 = 4;
    const OTHER: u8 = 5;

    /// A lookup's reply: the status header, then the encoded addresses.
    pub(super) type Reply = ([u8; 5], Vec<u8>);

    /// One `getaddrinfo` for a NUL-terminated name: the reply header and the
    /// encoded addresses.
    pub(super) fn lookup(name: &[u8]) -> Reply {
        let header = |status: u8, code: i32| {
            let code = code.to_be_bytes();
            [status, code[0], code[1], code[2], code[3]]
        };
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
        let status = match status {
            0 => FOUND,
            EAI_NONAME | EAI_NODATA => NONE,
            EAI_AGAIN => AGAIN,
            EAI_MEMORY => MEMORY,
            EAI_SYSTEM => {
                let code = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
                return (header(SYSTEM, code), Vec::new());
            }
            _ => OTHER,
        };
        if status != FOUND {
            return (header(status, 0), Vec::new());
        }
        let mut out = Vec::new();
        let mut outcome = Ok(());
        let mut entry = list;
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
        match outcome {
            Ok(()) => (header(FOUND, 0), out),
            Err(_) => (header(MEMORY, 0), Vec::new()),
        }
    }

    /// Sends all of `data`; false once the reader has gone (EPIPE) or failed.
    fn send_all(fd: &OwnedFd, data: &[u8]) -> bool {
        let mut sent = 0;
        while sent < data.len() {
            // SAFETY: a live slice and the sender's own descriptor.
            let count = unsafe {
                send(
                    fd.as_raw_fd(),
                    data[sent..].as_ptr(),
                    data.len() - sent,
                    MSG_NOSIGNAL,
                )
            };
            if count <= 0 {
                return false;
            }
            sent += count as usize;
        }
        true
    }

    struct Job {
        name: Vec<u8>,
        writer: OwnedFd,
        lookup: fn(&[u8]) -> Reply,
    }

    /// Runs on a detached native thread, not `std::thread`: std frees a
    /// thread's handle and result packet as the thread exits, after anything
    /// its closure can signal. Here every Rust allocation of the lookup is
    /// freed before `LOOKUPS` drops, so allocation checks can wait on it
    /// (issue #129).
    unsafe extern "C" fn run_lookup(job: *mut c_void) -> *mut c_void {
        // SAFETY: `start_lookup` gave this thread sole ownership of the job.
        let job =
            unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(job.cast::<Job>(), 1)) };
        let Job {
            name,
            writer,
            lookup,
        } = &job[0];
        let (header, addresses) = lookup(name);
        let _ = send_all(writer, &header) && send_all(writer, &addresses);
        drop((job, header, addresses));
        LOOKUPS.fetch_sub(1, Ordering::AcqRel);
        std::ptr::null_mut()
    }

    /// Blocks until every lookup thread, abandoned ones included, has freed
    /// its memory.
    #[cfg(any(test, all(feature = "allocation-checks", plenty_runtime_embedded)))]
    pub(crate) fn wait_for_lookups() {
        while LOOKUPS.load(Ordering::Acquire) != 0 {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// Starts `lookup` on a helper thread and returns the read end of a
    /// socket pair that carries its reply and then end of input. The caller
    /// waits on it with a deadline and token; dropping it abandons the
    /// lookup, whose thread then exits once the resolver returns.
    pub(super) fn start_lookup(host: &str, lookup: fn(&[u8]) -> Reply) -> Result<OwnedFd, Error> {
        if host.is_empty() || host.contains('\0') {
            return Err(Kind::InvalidInput.into());
        }
        let mut name = Vec::new();
        name.try_reserve(host.len() + 1)
            .map_err(|_| AllocError::OutOfMemory)?;
        name.extend_from_slice(host.as_bytes());
        name.push(0);
        let again = || Error::from(std::io::Error::from_raw_os_error(EAGAIN));
        let finished = || LOOKUPS.fetch_sub(1, Ordering::AcqRel);
        if LOOKUPS.fetch_add(1, Ordering::AcqRel) >= MAX_LOOKUPS {
            finished();
            return Err(again());
        }
        let mut ends = [0i32; 2];
        // SAFETY: room for two descriptors.
        if unsafe { socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, ends.as_mut_ptr()) } < 0 {
            finished();
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: socketpair returned two fresh descriptors owned here only.
        let (reader, writer) =
            unsafe { (OwnedFd::from_raw_fd(ends[0]), OwnedFd::from_raw_fd(ends[1])) };
        let mut job = Vec::new();
        if job.try_reserve_exact(1).is_err() {
            finished();
            return Err(AllocError::OutOfMemory.into());
        }
        job.push(Job {
            name,
            writer,
            lookup,
        });
        let job = Box::into_raw(job.into_boxed_slice());
        // SAFETY: glibc's pthread_attr_t fits the zeroed, aligned storage;
        // the job is handed to the new thread, or reclaimed if none starts.
        let status = unsafe {
            let mut attributes: Attributes = std::mem::zeroed();
            let mut status = pthread_attr_init(&mut attributes);
            if status == 0 {
                status = pthread_attr_setstacksize(&mut attributes, LOOKUP_STACK);
            }
            if status == 0 {
                status = pthread_attr_setdetachstate(&mut attributes, PTHREAD_CREATE_DETACHED);
            }
            if status == 0 {
                let mut thread = 0usize;
                status = pthread_create(
                    &mut thread,
                    (&raw const attributes).cast(),
                    run_lookup,
                    job.cast(),
                );
            }
            pthread_attr_destroy(&mut attributes);
            status
        };
        if status != 0 {
            // SAFETY: no thread received the job.
            drop(unsafe { Box::from_raw(job) });
            finished();
            return Err(again());
        }
        Ok(reader)
    }

    // Inlined so each operation's entry symbol keeps only its own arm.
    #[inline(always)]
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
                413 => match b {
                    0 => address(file(a)?, false, output),
                    1 => address(file(a)?, true, output),
                    _ => Err(Kind::InvalidInput.into()),
                },
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
                    let bytes = byte_storage::source(a);
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
                418 => {
                    let text = a as *const crate::strings::Text;
                    // The helper thread reads the name and frees the job,
                    // possibly after an actor's heap was discarded by a
                    // trap, so both come from System.
                    let host = crate::strings::utf8(&text);
                    let reader = crate::actor_heap::outside(|| start_lookup(host, lookup))?;
                    crate::files::adopt_socket(std::fs::File::from(reader), output)
                }
                _ => crate::fail("invalid network operation"),
            }
        }
    }
    #[cfg(test)]
    mod socket_tests {
        use super::*;
        use crate::{files, memory};

        static FILE: Type = Type {
            inline_range: false,
            inline_bytes: 0,
            kind: b'F',
            affine: true,
            reflexive: true,
            key: None,
            value: None,
            name: "",
            variants: &[],
            drop: None,
            wrapper: None,
            plan: None,
            redacted: false,
            transparent: false,
        };

        struct Owner(u128);

        impl Owner {
            fn pointer(&mut self) -> *mut files::File {
                self.0 as *mut files::File
            }

            fn fd(&self) -> i32 {
                // SAFETY: this test owns the initialized File until Drop.
                unsafe { files::descriptor(self.0 as *const files::File) }.unwrap()
            }
        }

        impl Drop for Owner {
            fn drop(&mut self) {
                // SAFETY: release this test's sole ownership exactly once.
                unsafe { memory::plenty_release(self.0 as *mut memory::Header) };
            }
        }

        fn pair() -> (Owner, Owner) {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port().to_be_bytes();
            let mut client =
                Owner(open(&[4, port[0], port[1], 127, 0, 0, 1], CONNECT, 0, &FILE).unwrap());
            // SAFETY: the test owns this live File; bound setup to five seconds.
            let ready = unsafe { files::wait_ready(client.pointer(), 2, 5_000_000_000) }.unwrap();
            assert_ne!(ready, 0);
            finish_connect(client.fd()).unwrap();
            let server = Owner(accept(listener.as_raw_fd(), &FILE).unwrap());
            (client, server)
        }

        fn assert_nonblocking(fd: i32) {
            unsafe extern "C" {
                fn fcntl(fd: i32, command: i32, ...) -> i32;
            }
            // SAFETY: integer get commands on a live descriptor.
            let status = unsafe { fcntl(fd, 3) };
            assert!(status >= 0);
            assert_ne!(
                status & SOCK_NONBLOCK,
                0,
                "TCP descriptor must stay nonblocking"
            );
            // SAFETY: F_GETFD takes no trailing argument.
            assert_eq!(
                unsafe { fcntl(fd, 1) } & 1,
                1,
                "TCP descriptor must be close-on-exec"
            );
        }

        fn is_again(error: Error) -> bool {
            matches!(error, Error::System(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        }

        #[test]
        fn connected_and_accepted_streams_never_block_after_stale_readiness() {
            let (mut client, mut server) = pair();
            for owner in [&mut client, &mut server] {
                assert_nonblocking(owner.fd());
                // SAFETY: exclusive owners and a valid one-byte destination.
                assert!(
                    unsafe { files::read_bytes(owner.pointer(), &mut [0]) }.is_err_and(is_again)
                );
            }
            // SAFETY: live exclusive owners and valid byte buffers. Drain the
            // byte after a readiness report, then model a second stale attempt.
            unsafe {
                assert_eq!(files::write_bytes(client.pointer(), b"x").unwrap(), 1);
                assert_ne!(
                    files::wait_ready(server.pointer(), 1, 5_000_000_000).unwrap(),
                    0
                );
                assert_eq!(files::read_bytes(server.pointer(), &mut [0]).unwrap(), 1);
                assert!(files::read_bytes(server.pointer(), &mut [0]).is_err_and(is_again));
            }
        }

        #[test]
        fn a_small_full_send_buffer_reports_would_block() {
            let (mut client, server) = pair();
            assert_nonblocking(client.fd());
            option(client.fd(), SOL_SOCKET, 7, 4096).unwrap(); // SO_SNDBUF
            option(server.fd(), SOL_SOCKET, 8, 4096).unwrap(); // SO_RCVBUF
            let block = [0; 4096];
            for _ in 0..4096 {
                // SAFETY: this test owns the live socket and source buffer.
                match unsafe { files::write_bytes(client.pointer(), &block) } {
                    Ok(count) => assert!(count > 0 && count <= block.len() as u128),
                    Err(error) => {
                        assert!(is_again(error));
                        return;
                    }
                }
            }
            panic!("an idle peer must exhaust the bounded send buffer");
        }
    }
}

#[cfg(all(test, target_os = "linux", target_pointer_width = "64"))]
mod tests {
    use super::native::{Reply, start_lookup, wait_for_lookups};
    use std::io::Read;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    static SLOW_DONE: AtomicBool = AtomicBool::new(false);

    fn slow(_: &[u8]) -> Reply {
        std::thread::sleep(Duration::from_millis(300));
        SLOW_DONE.store(true, Ordering::Release);
        ([0; 5], vec![4, 0, 80, 127, 0, 0, 1])
    }

    fn quick(name: &[u8]) -> Reply {
        assert_eq!(name, b"example\0");
        ([0; 5], vec![4, 0, 80, 127, 0, 0, 1])
    }

    #[test]
    fn a_lookup_reply_ends_with_end_of_input() {
        let reader = start_lookup("example", quick).unwrap();
        let mut reply = Vec::new();
        std::fs::File::from(reader).read_to_end(&mut reply).unwrap();
        assert_eq!(reply, [0, 0, 0, 0, 0, 4, 0, 80, 127, 0, 0, 1]);
        assert!(start_lookup("", quick).is_err());
        assert!(start_lookup("a\0b", quick).is_err());
    }

    #[test]
    fn an_abandoned_lookup_finishes_without_its_reader() {
        let started = Instant::now();
        let reader = start_lookup("slow", slow).unwrap();
        // Closing the reader before the reply exists must not wait for the
        // lookup, and its later send must not raise SIGPIPE.
        drop(reader);
        assert!(started.elapsed() < Duration::from_millis(250));
        wait_for_lookups();
        assert!(SLOW_DONE.load(Ordering::Acquire));
    }
}

#[cfg(all(
    target_os = "linux",
    target_pointer_width = "64",
    feature = "allocation-checks",
    plenty_runtime_embedded
))]
pub(crate) use native::wait_for_lookups;

/// Operands are live values of the statically checked signature.
// Inlined so each operation's entry symbol keeps only its own arm.
#[inline(always)]
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
