//! Worker-count estimates without a monitoring dependency or heap allocation.
//! Linux uses OS affinity plus bounded reads of the conventional cgroup mounts.
//! The Rust standard library's cgroup discovery allocates infallibly, so it
//! cannot be used on Linux under Plenty's allocation-failure contract.

const UNAVAILABLE: i64 = -1;
const UNSUPPORTED: i64 = -2;
const LIMIT: i64 = -3;

pub(crate) fn call() -> i64 {
    let result = estimate().unwrap_or_else(|status| status);
    debug_assert!(result > 0 || [UNAVAILABLE, UNSUPPORTED, LIMIT].contains(&result));
    result
}

#[cfg(target_os = "linux")]
fn estimate() -> Result<i64, i64> {
    // Linux accepts a larger affinity buffer than cpu_set_t. A fixed 8 KiB
    // covers 65536 logical CPUs; EINVAL fails rather than overcounting through
    // a machine-wide sysconf fallback. The u64 array provides its alignment.
    let mut mask = [0u64; 1024];
    // SAFETY: mask is live writable storage of the supplied byte length.
    if unsafe { libc::sched_getaffinity(0, size_of_val(&mask), mask.as_mut_ptr().cast()) } != 0 {
        return Err(
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINVAL) {
                LIMIT
            } else {
                UNAVAILABLE
            },
        );
    }
    let affinity: u64 = mask.iter().map(|word| u64::from(word.count_ones())).sum();
    if affinity == 0 {
        return Err(UNAVAILABLE);
    }
    Ok(affinity.min(linux::quota()?.unwrap_or(u64::MAX)).max(1) as i64)
}

// These standard-library backends use native scalar OS queries, without the
// allocating Linux cgroup scan. Their estimates retain Rust's platform limits.
#[cfg(any(target_vendor = "apple", windows))]
fn estimate() -> Result<i64, i64> {
    match std::thread::available_parallelism() {
        Ok(count) => i64::try_from(count.get()).map_err(|_| LIMIT),
        Err(error) if error.kind() == std::io::ErrorKind::Unsupported => Err(UNSUPPORTED),
        Err(_) => Err(UNAVAILABLE),
    }
}

#[cfg(not(any(target_os = "linux", target_vendor = "apple", windows)))]
fn estimate() -> Result<i64, i64> {
    Err(UNSUPPORTED)
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{LIMIT, UNAVAILABLE};
    use std::fs::File;
    use std::io::{self, Read};
    use std::os::fd::FromRawFd;

    const MAX_PATH: usize = 4096;

    fn open(path: &[u8], flags: i32) -> Result<Option<File>, i64> {
        if path.len() > MAX_PATH {
            return Err(LIMIT);
        }
        if path.contains(&0) {
            return Err(UNAVAILABLE);
        }
        // File::open converts long paths with an infallible CString allocation.
        // Our bounded NUL-terminated buffer avoids that hidden allocation too.
        let mut terminated = [0u8; MAX_PATH + 1];
        terminated[..path.len()].copy_from_slice(path);
        // SAFETY: terminated is NUL terminated and no creation flag is passed.
        let fd = unsafe {
            libc::open(
                terminated.as_ptr().cast(),
                libc::O_RDONLY | libc::O_CLOEXEC | flags,
            )
        };
        if fd < 0 {
            if io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
                return Ok(None);
            }
            return Err(UNAVAILABLE);
        }
        // SAFETY: a successful open transfers this unique descriptor to File.
        Ok(Some(unsafe { File::from_raw_fd(fd) }))
    }

    // Every input buffer has one probe byte, so exact-limit EOF is success.
    fn read(path: &[u8], buffer: &mut [u8]) -> Result<Option<usize>, i64> {
        let Some(mut file) = open(path, 0)? else {
            return Ok(None);
        };
        let mut used = 0;
        while used < buffer.len() {
            match file.read(&mut buffer[used..]) {
                Ok(0) => return Ok(Some(used)),
                Ok(count) => used += count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(UNAVAILABLE),
            }
        }
        Err(LIMIT)
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Version {
        V1,
        V2,
    }

    fn membership(text: &[u8]) -> Result<Option<(Version, &[u8])>, i64> {
        let mut found = None;
        for line in text
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            let mut parts = line.splitn(3, |byte| *byte == b':');
            let _id = parts.next().ok_or(UNAVAILABLE)?;
            let controllers = parts.next().ok_or(UNAVAILABLE)?;
            let path = parts.next().ok_or(UNAVAILABLE)?;
            let version = if controllers
                .split(|byte| *byte == b',')
                .any(|name| name == b"cpu")
            {
                Version::V1
            } else if controllers.is_empty() {
                Version::V2
            } else {
                continue;
            };
            // Escaped paths and namespace-relative parent paths cannot safely
            // be resolved against the conventional mount. Report a failure.
            if !path.starts_with(b"/")
                || path.contains(&b'\\')
                || path
                    .split(|byte| *byte == b'/')
                    .any(|name| name == b".." || name == b".")
            {
                return Err(UNAVAILABLE);
            }
            if !matches!(found, Some((Version::V1, _))) {
                found = Some((version, path));
            }
        }
        Ok(found)
    }

    fn number(text: &[u8]) -> Result<u64, i64> {
        if text.is_empty() || !text.iter().all(u8::is_ascii_digit) {
            return Err(UNAVAILABLE);
        }
        text.iter().try_fold(0u64, |value, byte| {
            value
                .checked_mul(10)
                .and_then(|value| value.checked_add(u64::from(byte - b'0')))
                .ok_or(UNAVAILABLE)
        })
    }

    fn quota_value(limit: &[u8], period: &[u8]) -> Result<Option<u64>, i64> {
        let period = number(period)?;
        if period == 0 {
            return Err(UNAVAILABLE);
        }
        if limit == b"max" || limit == b"-1" {
            Ok(None)
        } else {
            // Whole workers use steady-state capacity, floored but at least
            // one worker even for a fractional CPU quota.
            Ok(Some((number(limit)? / period).max(1)))
        }
    }

    fn trim(text: &[u8]) -> &[u8] {
        text.trim_ascii()
    }

    fn file(
        path: &mut [u8; MAX_PATH],
        used: usize,
        name: &[u8],
        buffer: &mut [u8],
    ) -> Result<Option<usize>, i64> {
        let end = used
            .checked_add(name.len())
            .filter(|end| *end <= path.len())
            .ok_or(LIMIT)?;
        path[used..end].copy_from_slice(name);
        read(&path[..end], buffer)
    }

    fn hierarchy(mount: &[u8], group: &[u8], version: Version) -> Result<Option<u64>, i64> {
        let mut path = [0u8; MAX_PATH];
        let mut used = mount
            .len()
            .checked_add(group.len())
            .filter(|len| *len < MAX_PATH)
            .ok_or(LIMIT)?;
        path[..mount.len()].copy_from_slice(mount);
        path[mount.len()..used].copy_from_slice(group);
        while used > mount.len() && path[used - 1] == b'/' {
            used -= 1;
        }
        open(&path[..used], libc::O_DIRECTORY)?.ok_or(UNAVAILABLE)?;
        let mut quota = None;
        loop {
            let mut value = [0u8; 513];
            let parsed = match version {
                Version::V2 => match file(&mut path, used, b"/cpu.max", &mut value)? {
                    Some(len) => {
                        let mut words = value[..len]
                            .split(u8::is_ascii_whitespace)
                            .filter(|word| !word.is_empty());
                        let limit = words.next().ok_or(UNAVAILABLE)?;
                        let period = words.next().ok_or(UNAVAILABLE)?;
                        if words.next().is_some() {
                            return Err(UNAVAILABLE);
                        }
                        quota_value(limit, period)?
                    }
                    None => None,
                },
                Version::V1 => match file(&mut path, used, b"/cpu.cfs_quota_us", &mut value)? {
                    Some(len) => {
                        let mut period = [0u8; 513];
                        let count = file(&mut path, used, b"/cpu.cfs_period_us", &mut period)?
                            .ok_or(UNAVAILABLE)?;
                        quota_value(trim(&value[..len]), trim(&period[..count]))?
                    }
                    None => return Err(UNAVAILABLE),
                },
            };
            if let Some(count) = parsed {
                quota = Some(quota.map_or(count, |old: u64| old.min(count)));
            }
            if used == mount.len() {
                break;
            }
            used = path[..used]
                .iter()
                .rposition(|byte| *byte == b'/')
                .ok_or(UNAVAILABLE)?
                .max(mount.len());
        }
        Ok(quota)
    }

    pub(super) fn quota() -> Result<Option<u64>, i64> {
        let mut data = [0u8; 65537];
        let len = read(b"/proc/self/cgroup", &mut data)?.ok_or(UNAVAILABLE)?;
        let Some((version, group)) = membership(&data[..len])? else {
            return Ok(None);
        };
        match version {
            Version::V2 => {
                let mut probe = [0u8; 65537];
                // Verify the conventional mount is cgroup v2, even at root
                // where cpu.max may legitimately be absent.
                read(b"/sys/fs/cgroup/cgroup.controllers", &mut probe)?.ok_or(UNAVAILABLE)?;
                hierarchy(b"/sys/fs/cgroup", group, version)
            }
            Version::V1 => {
                for mount in [
                    b"/sys/fs/cgroup/cpu".as_slice(),
                    b"/sys/fs/cgroup/cpu,cpuacct",
                ] {
                    if open(mount, libc::O_DIRECTORY)?.is_some() {
                        return hierarchy(mount, group, version);
                    }
                }
                Err(UNAVAILABLE)
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::os::unix::ffi::OsStrExt;

        struct Fixture(std::path::PathBuf);

        impl Fixture {
            fn new() -> Self {
                let path = std::env::temp_dir().join(format!("plenty-cpu-{}", std::process::id()));
                std::fs::create_dir(&path).unwrap();
                Self(path)
            }

            fn bytes(&self) -> &[u8] {
                self.0.as_os_str().as_bytes()
            }
        }

        impl Drop for Fixture {
            fn drop(&mut self) {
                std::fs::remove_dir_all(&self.0).unwrap();
            }
        }

        #[test]
        fn ancestors_unlimited_quotas_and_bounded_files() {
            let root = Fixture::new();
            std::fs::create_dir(root.0.join("pool")).unwrap();
            std::fs::create_dir(root.0.join("pool/job")).unwrap();
            std::fs::write(root.0.join("cpu.max"), "max 100000\n").unwrap();
            std::fs::write(root.0.join("pool/cpu.max"), "250000 100000\n").unwrap();
            std::fs::write(root.0.join("pool/job/cpu.max"), "900000 100000\n").unwrap();
            assert_eq!(
                hierarchy(root.bytes(), b"/pool/job", Version::V2),
                Ok(Some(2))
            );
            std::fs::write(root.0.join("pool/cpu.max"), "50000 100000\n").unwrap();
            assert_eq!(
                hierarchy(root.bytes(), b"/pool/job", Version::V2),
                Ok(Some(1))
            );
            assert_eq!(hierarchy(root.bytes(), b"/", Version::V2), Ok(None));
            std::fs::write(root.0.join("pool/job/cpu.max"), "bad\n").unwrap();
            assert_eq!(
                hierarchy(root.bytes(), b"/pool/job", Version::V2),
                Err(UNAVAILABLE)
            );
            assert_eq!(
                hierarchy(root.bytes(), b"/missing", Version::V2),
                Err(UNAVAILABLE)
            );
            std::fs::write(root.0.join("pool/job/cpu.max"), [b'0'; 513]).unwrap();
            assert_eq!(
                hierarchy(root.bytes(), b"/pool/job", Version::V2),
                Err(LIMIT)
            );
            assert_eq!(
                hierarchy(root.bytes(), &[b'x'; 4096], Version::V2),
                Err(LIMIT)
            );
            for directory in ["", "pool/", "pool/job/"] {
                std::fs::write(root.0.join(format!("{directory}cpu.cfs_quota_us")), "-1\n")
                    .unwrap();
                std::fs::write(
                    root.0.join(format!("{directory}cpu.cfs_period_us")),
                    "100000\n",
                )
                .unwrap();
            }
            std::fs::write(root.0.join("pool/cpu.cfs_quota_us"), "300000\n").unwrap();
            assert_eq!(
                hierarchy(root.bytes(), b"/pool/job", Version::V1),
                Ok(Some(3))
            );
            let file = root.0.join("bounded");
            std::fs::write(&file, b"four").unwrap();
            assert_eq!(
                read(file.as_os_str().as_bytes(), &mut [0u8; 5]),
                Ok(Some(4))
            );
            assert_eq!(read(file.as_os_str().as_bytes(), &mut [0u8; 4]), Err(LIMIT));
        }

        #[test]
        fn membership_prefers_explicit_cpu_controller_and_rejects_unsafe_paths() {
            assert_eq!(
                membership(b"0::/jobs\n2:cpu,cpuacct:/pool\n").unwrap(),
                Some((Version::V1, b"/pool".as_slice()))
            );
            assert_eq!(
                membership(b"2:cpu:/pool\n0::/jobs\n").unwrap(),
                Some((Version::V1, b"/pool".as_slice()))
            );
            assert_eq!(membership(b"2:memory:/pool\n").unwrap(), None);
            for text in [
                b"0::/../pool".as_slice(),
                b"0::relative",
                b"0::/escaped\\040path",
                b"bad",
            ] {
                assert_eq!(membership(text), Err(UNAVAILABLE));
            }
        }

        #[test]
        fn quotas_floor_fractional_capacity_without_zero_or_overflow() {
            assert_eq!(quota_value(b"250000", b"100000"), Ok(Some(2)));
            assert_eq!(quota_value(b"50000", b"100000"), Ok(Some(1)));
            assert_eq!(quota_value(b"max", b"100000"), Ok(None));
            assert_eq!(quota_value(b"-1", b"100000"), Ok(None));
            for (limit, period) in [
                (b"1".as_slice(), b"0".as_slice()),
                (b"x", b"1"),
                (b"18446744073709551616", b"1"),
            ] {
                assert_eq!(quota_value(limit, period), Err(UNAVAILABLE));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_result_is_positive_or_a_documented_failure() {
        let result = call();
        assert!(result > 0 || [UNAVAILABLE, UNSUPPORTED, LIMIT].contains(&result));
    }
}
