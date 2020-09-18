// Types of surface roles
//
// Surfaces can be used for multiple things, and the
// role specifies how we are going to use a surface.
// (window vs cursor vs ...)
//
// Austin Shafer 2020
use super::xdg_shell;
use super::wl_subcompositor::SubSurface;

use std::rc::Rc;
use std::cell::RefCell;

pub enum Role {
    // This window belongs to a parent. See atmosphere
    subsurface(Rc<RefCell<SubSurface>>),
    // This window is being controlled by wl_shell (deprecated)
    wl_shell_toplevel,
    // This window is being controlled by xdg_shell
    xdg_shell_toplevel(Rc<RefCell<xdg_shell::ShellSurface>>),
}
