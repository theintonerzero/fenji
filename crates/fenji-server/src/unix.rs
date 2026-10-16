//! The C library's calls that std, tokio and rustix don't wrap, on Linux and macOS: the one place
//! the server writes `unsafe`. Each asks the system something and writes only into memory of its
//! own.
#![allow(unsafe_code)]

/// Whether SIGHUP came ignored, as `nohup` starts a program so that it outlives its terminal (the
/// hosting plan's final review, Important 1). Read before the server catches SIGHUP: catching it
/// replaces what was there.
pub(crate) fn hangup_ignored() -> bool {
    // SAFETY: an all-zero `sigaction` is a valid one (integers, a signal set, and on Linux a null
    // restorer), and `sigaction` given no new action only writes the current one into it.
    unsafe {
        let mut was: libc::sigaction = std::mem::zeroed();
        libc::sigaction(libc::SIGHUP, std::ptr::null(), &mut was) == 0
            && was.sa_sigaction == libc::SIG_IGN
    }
}

/// The name of the user `uid`, if the system has one, for a hint naming whom to run as (the
/// hosting plan's final review, Minor 1).
pub(crate) fn user_name(uid: u32) -> Option<String> {
    let mut buf = vec![0u8; 1024];
    loop {
        let mut found: *mut libc::passwd = std::ptr::null_mut();
        // SAFETY: an all-zero `passwd` is a valid one (integers and null pointers), and
        // `getpwuid_r` writes only into it, into `buf` up to its length, and into `found`.
        let (entry, read) = unsafe {
            let mut entry: libc::passwd = std::mem::zeroed();
            let read = libc::getpwuid_r(
                uid,
                &mut entry,
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut found,
            );
            (entry, read)
        };
        if read == libc::ERANGE && buf.len() < 1 << 20 {
            buf.resize(buf.len() * 2, 0);
            continue;
        }
        if read != 0 || found.is_null() || entry.pw_name.is_null() {
            return None;
        }
        // SAFETY: found, `pw_name` points to a name ending in NUL in `buf`, which is still here.
        let name = unsafe { std::ffi::CStr::from_ptr(entry.pw_name) };
        return Some(name.to_string_lossy().into_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_user_is_named_as_the_system_names_it() {
        assert_eq!(user_name(0).as_deref(), Some("root"));
        assert_eq!(user_name(4_000_000_123), None, "no such user");
    }

    /// Whether the kernel lists SIGHUP among this process's ignored signals: bit 0 of
    /// /proc/self/status's `SigIgn` mask, SIGHUP being signal 1.
    #[cfg(target_os = "linux")]
    fn kernel_says_ignored() -> bool {
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        let mask = status
            .lines()
            .find_map(|l| l.strip_prefix("SigIgn:"))
            .unwrap()
            .trim();
        u64::from_str_radix(mask, 16).unwrap() & 1 == 1
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn whether_sighup_came_ignored_is_what_the_kernel_says() {
        // The test runner may itself run under nohup, so the answer is the kernel's either way.
        assert_eq!(hangup_ignored(), kernel_says_ignored());
    }
}
