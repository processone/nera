//! Wakeable timed wait for the timer thread.
//!
//! `wake` leaves a pending wake behind: a wake that lands before `wait`
//! starts makes that wait return at once. None is lost between the timer
//! thread reading its next deadline and going to sleep.
//!
//! macOS postpones the ordinary timed waits of a process running at
//! background or utility priority (timer coalescing): a 37 ms condition
//! variable wait could last over 100 ms. There, exact waits use a kqueue
//! timer marked `NOTE_CRITICAL`, which the kernel fires on time at any
//! priority.

pub(crate) use imp::Waiter;

/// How closely a wait must honor its timeout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Precision {
    /// A program-visible deadline, such as `receive ... after`.
    Exact,
    /// A fallback bound the OS may stretch to save energy.
    Relaxed,
}

#[cfg(target_os = "macos")]
mod imp {
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::time::Duration;

    use super::Precision;

    const WAKE: usize = 1;
    const DEADLINE: usize = 2;

    pub(crate) struct Waiter {
        kq: OwnedFd,
    }

    fn event(ident: usize, filter: i16, flags: u16, fflags: u32, data: isize) -> libc::kevent {
        libc::kevent { ident, filter, flags, fflags, data, udata: std::ptr::null_mut() }
    }

    impl Waiter {
        pub(crate) fn new() -> Self {
            // SAFETY: kqueue takes no arguments; a non-negative result is a
            // fresh descriptor that only this Waiter owns and closes.
            let kq = unsafe {
                let fd = libc::kqueue();
                assert!(fd >= 0, "failed to create the timer kqueue: {}", io::Error::last_os_error());
                libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
                OwnedFd::from_raw_fd(fd)
            };
            let waiter = Waiter { kq };
            // EV_CLEAR: a trigger stays pending until one wait retrieves it.
            waiter.change(event(WAKE, libc::EVFILT_USER, libc::EV_ADD | libc::EV_CLEAR, 0, 0))
                .expect("failed to register the timer wake event");
            waiter
        }

        fn change(&self, change: libc::kevent) -> io::Result<()> {
            // SAFETY: one valid change, no event buffer, no timeout.
            let n = unsafe {
                libc::kevent(self.kq.as_raw_fd(), &change, 1, std::ptr::null_mut(), 0, std::ptr::null())
            };
            if n < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
        }

        pub(crate) fn wake(&self) {
            // Only fails if the descriptor is invalid, which ownership rules out.
            let _ = self.change(event(WAKE, libc::EVFILT_USER, 0, libc::NOTE_TRIGGER, 0));
        }

        /// Return once woken or after `timeout`, whichever comes first.
        /// May also return early; callers recheck their condition.
        pub(crate) fn wait(&self, timeout: Duration, precision: Precision) {
            let mut fflags = libc::NOTE_NSECONDS;
            if precision == Precision::Exact {
                fflags |= libc::NOTE_CRITICAL;
            }
            let nanos = isize::try_from(timeout.as_nanos()).unwrap_or(isize::MAX).max(1);
            // Re-adding the one-shot timer replaces one left armed by a wake.
            let arm = event(DEADLINE, libc::EVFILT_TIMER, libc::EV_ADD | libc::EV_ONESHOT, fflags, nanos);
            let mut fired = [event(0, 0, 0, 0, 0); 2];
            // SAFETY: one valid change and a writable buffer of two events.
            let n = unsafe {
                libc::kevent(self.kq.as_raw_fd(), &arm, 1, fired.as_mut_ptr(), 2, std::ptr::null())
            };
            let failed = if n < 0 {
                io::Error::last_os_error().kind() != io::ErrorKind::Interrupted
            } else {
                fired[..n as usize].iter().any(|e| e.flags & libc::EV_ERROR != 0)
            };
            if failed {
                // Not expected on supported macOS. Poll instead of spinning or
                // sleeping through wakes.
                std::thread::sleep(timeout.min(Duration::from_millis(1)));
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::time::Duration;

    use parking_lot::{Condvar, Mutex};

    use super::Precision;

    pub(crate) struct Waiter {
        woken: Mutex<bool>,
        changed: Condvar,
    }

    impl Waiter {
        pub(crate) fn new() -> Self {
            Waiter { woken: Mutex::new(false), changed: Condvar::new() }
        }

        pub(crate) fn wake(&self) {
            *self.woken.lock() = true;
            self.changed.notify_one();
        }

        /// Return once woken or after `timeout`, whichever comes first.
        /// May also return early; callers recheck their condition.
        pub(crate) fn wait(&self, timeout: Duration, _precision: Precision) {
            let mut woken = self.woken.lock();
            if !*woken {
                self.changed.wait_for(&mut woken, timeout);
            }
            *woken = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    const LONG: Duration = Duration::from_secs(30);

    #[test]
    fn wake_before_wait_is_kept() {
        for precision in [Precision::Exact, Precision::Relaxed] {
            let waiter = Waiter::new();
            waiter.wake();
            let started = Instant::now();
            waiter.wait(LONG, precision);
            assert!(started.elapsed() < LONG / 3, "a wake sent before the wait was lost");
        }
    }

    #[test]
    fn wake_is_consumed_by_one_wait() {
        let waiter = Waiter::new();
        waiter.wake();
        waiter.wait(LONG, Precision::Relaxed);
        let started = Instant::now();
        waiter.wait(Duration::from_millis(20), Precision::Exact);
        assert!(started.elapsed() >= Duration::from_millis(15), "an old wake ended a later wait");
    }

    #[test]
    fn wake_from_another_thread_ends_a_wait() {
        let waiter = std::sync::Arc::new(Waiter::new());
        let sleeper = std::sync::Arc::clone(&waiter);
        let started = Instant::now();
        let handle = std::thread::spawn(move || sleeper.wait(LONG, Precision::Exact));
        waiter.wake();
        handle.join().unwrap();
        assert!(started.elapsed() < LONG / 3, "wake did not end the wait");
    }
}
