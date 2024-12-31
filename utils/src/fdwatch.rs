// Helper class for watching file descriptors
// OS-compatibility layer
//
// Austin Shafer - 2020
extern crate nix;

#[cfg(target_os = "freebsd")]
use nix::sys::event::*;
#[cfg(not(target_os = "freebsd"))]
use nix::sys::select::*;

#[cfg(not(target_os = "freebsd"))]
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::io::RawFd;

// =============================================
// kqueue version
// =============================================

// A file descriptor watcher
#[cfg(target_os = "freebsd")]
pub struct FdWatch {
    // The kqueue fd
    fdw_kq: Kqueue,
    // Events to watch
    fdw_events: Vec<KEvent>,
    fdw_fds: Vec<RawFd>,
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
            fdw_kq: Kqueue::new().expect("Could not create kqueue"),
            fdw_events: Vec::new(),
            fdw_fds: Vec::new(),
        }
    }

    pub fn add_fd(&mut self, fd: RawFd) {
        let kev = FdWatch::read_fd_kevent(fd);
        self.fdw_events.push(kev);
        self.fdw_fds.push(fd);
    }

    pub fn remove_fd(&mut self, fd: RawFd) {
        let index = self
            .fdw_fds
            .iter()
            .position(|f| *f == fd)
            .expect("FdWatch: Could not find requested fd");
        self.fdw_events.remove(index);
        self.fdw_fds.remove(index);
    }

    pub fn register_events(&mut self) {
        // Register our kevent with the kqueue to receive updates
        self.fdw_kq
            .kevent(self.fdw_events.as_slice(), &mut [], None)
            .expect("Could not register watch event with kqueue");
    }

    // returns true if something is ready to be read
    pub fn wait_for_events(&mut self, timeout: Option<usize>) -> bool {
        self.fdw_kq
            .kevent(
                &[],
                self.fdw_events.as_mut_slice(),
                timeout.map(|t| {
                    nix::sys::time::TimeSpec::from_duration(std::time::Duration::from_millis(
                        t as u64,
                    ))
                    .as_ref()
                    .clone()
                }),
            )
            .is_ok()
    }
}

// =============================================
// Generic select
// =============================================

// A file descriptor watcher
#[cfg(not(target_os = "freebsd"))]
pub struct FdWatch {
    // Events to watch
    fdw_events: Vec<OwnedFd>,
}

#[cfg(not(target_os = "freebsd"))]
impl FdWatch {
    pub fn new() -> FdWatch {
        FdWatch {
            fdw_events: Vec::new(),
        }
    }

    pub fn add_fd(&mut self, fd: RawFd) {
        unsafe {
            self.fdw_events.push(OwnedFd::from_raw_fd(fd));
        }
    }

    pub fn remove_fd(&mut self, fd: RawFd) {
        self.fdw_events.retain(|f| f.as_raw_fd() != fd);
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
            fdset.insert(fd.as_fd());
        }

        // add all of our fds to the readfd list
        let mut out = match timeout {
            Some(ms) => Some(nix::sys::time::TimeVal::milliseconds(ms as i64)),
            None => None,
        };
        select(None, Some(&mut fdset), None, None, out.as_mut()).is_ok()
    }
}
