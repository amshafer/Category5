// Helper class for watching file descriptors
// OS-compatibility layer
//
// Austin Shafer - 2020
extern crate nix;

#[cfg(not(target_os = "freebsd"))]
use nix::sys::select::*;

use nix::unistd::close;

#[cfg(target_os = "freebsd")]
use nix::sys::event::*;
use std::os::unix::io::RawFd;

// =============================================
// kqueue version
// =============================================

// A file descriptor watcher
#[cfg(target_os = "freebsd")]
pub struct FdWatch {
    // The kqueue fd
    fdw_kq: RawFd,
    // Events to watch
    fdw_events: Vec<KEvent>,
}

#[cfg(target_os = "freebsd")]
impl FdWatch {
    // Helper for creating an empty KEvent for kqueue
    // This is just for placeholders when we need
    // an initialized kevent
    #[allow(dead_code)]
    fn empty_kevent() -> KEvent {
        FdWatch::read_fd_kevent(0)
    }

    // Helper for creating a kevent for reading an fd
    fn read_fd_kevent(fd: RawFd) -> KEvent {
        KEvent::new(
            fd as usize,
            EventFilter::EVFILT_READ,
            EventFlag::EV_ADD,
            FilterFlag::all(),
            0,
            0,
        )
    }

    pub fn new() -> FdWatch {
        FdWatch {
            // Create a new kqueue
            fdw_kq: kqueue().expect("Could not create kqueue"),
            fdw_events: Vec::new(),
        }
    }

    pub fn add_fd(&mut self, fd: RawFd) {
        let kev = FdWatch::read_fd_kevent(fd);
        self.fdw_events.push(kev);
    }

    pub fn register_events(&mut self) {
        // Register our kevent with the kqueue to receive updates
        kevent(self.fdw_kq, self.fdw_events.as_slice(), &mut [], 0)
            .expect("Could not register watch event with kqueue");
    }

    // returns true if something is ready to be read
    pub fn wait_for_events(&mut self, timeout: Option<usize>) -> bool {
        match timeout {
            Some(ms) => kevent(self.fdw_kq, &[], self.fdw_events.as_mut_slice(), ms).is_ok(),
            None => kevent_ts(self.fdw_kq, &[], self.fdw_events.as_mut_slice(), None).is_ok(),
        }
    }
}

#[cfg(target_os = "freebsd")]
impl Drop for FdWatch {
    fn drop(&mut self) {
        close(self.fdw_kq).expect("Could not close FdWatch Kqueue fd");
    }
}

// =============================================
// Generic select
// =============================================

// A file descriptor watcher
#[cfg(not(target_os = "freebsd"))]
pub struct FdWatch {
    // Events to watch
    fdw_events: Vec<RawFd>,
}

#[cfg(not(target_os = "freebsd"))]
impl FdWatch {
    pub fn new() -> FdWatch {
        FdWatch {
            fdw_events: Vec::new(),
        }
    }

    pub fn add_fd(&mut self, fd: RawFd) {
        self.fdw_events.push(fd);
    }

    pub fn register_events(&mut self) {
        // noop since select doesn't need registration
    }

    // timeout in ms
    // returns true if something is ready to be read
    pub fn wait_for_events(&mut self, timeout: Option<usize>) -> bool {
        use crate::fdwatch::nix::sys::time::TimeValLike;

        let mut fdset = FdSet::new();
        for fd in self.fdw_events.iter() {
            fdset.insert(*fd);
        }

        // add all of our fds to the readfd list
        let mut out = match timeout {
            Some(ms) => Some(nix::sys::time::TimeVal::milliseconds(ms as i64)),
            None => None,
        };
        select(None, Some(&mut fdset), None, None, out.as_mut()).is_ok()
    }
}

#[cfg(not(target_os = "freebsd"))]
impl Drop for FdWatch {
    fn drop(&mut self) {
        for fd in self.fdw_events.iter() {
            close(*fd).expect("Could not close FdWatch fd");
        }
    }
}
