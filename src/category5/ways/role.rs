// Types of surface roles
//
// Surfaces can be used for multiple things, and the
// role specifies how we are going to use a surface.
// (window vs cursor vs ...)
//
// Austin Shafer 2020
use super::wl_subcompositor::SubSurface;
use super::xdg_shell;

use std::sync::{Arc, Mutex};

pub enum Role {
    // This window belongs to a parent. See atmosphere
    subsurface(Arc<Mutex<SubSurface>>),
    // This window is being controlled by wl_shell (deprecated)
    wl_shell_toplevel,
    // This window is being controlled by xdg_shell
    xdg_shell_toplevel(Arc<Mutex<xdg_shell::ShellSurface>>),
    xdg_shell_popup(Arc<Mutex<xdg_shell::ShellSurface>>),
}
