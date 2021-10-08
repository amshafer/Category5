// Helper class for watching file descriptors
// OS-compatibility layer
//
// Austin Shafer - 2020
extern crate nix;

#[cfg(target_os = "linux")]
use nix::sys::select::*;
#[cfg(target_os = "linux")]
use nix::sys::time::{TimeVal, TimeValLike};

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

    // timeout in ms
    // returns true if something is ready to be read
    pub fn wait_for_events(&mut self, timeout: usize) -> bool {
        kevent(self.fdw_kq, &[], self.fdw_events.as_mut_slice(), timeout).is_ok()
    }
}

// =============================================
// Generic select
// =============================================

// A file descriptor watcher
#[cfg(target_os = "linux")]
pub struct FdWatch {
    // Events to watch
    fdw_events: Vec<RawFd>,
}

#[cfg(target_os = "linux")]
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
    pub fn wait_for_events(&mut self, timeout: usize) -> bool {
        let mut fdset = FdSet::new();
        self.fdw_events.iter().map(|fd| fdset.insert(*fd));

        // add all of our fds to the readfd list
        select(
            None,
            Some(&mut fdset),
            None,
            None,
            Some(&mut TimeVal::milliseconds(timeout as i64)),
        )
        .is_ok()
    }
}
