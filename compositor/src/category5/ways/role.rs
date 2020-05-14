// Types of surface roles
//
// Surfaces can be used for multiple things, and the
// role specifies how we are going to use a surface.
// (window vs cursor vs ...)
//
// Austin Shafer 2020
use super::xdg_shell::ShellSurface;

use std::rc::Rc;
use std::cell::RefCell;

pub enum Role {
    xdg_shell_toplevel(Rc<RefCell<ShellSurface>>),
}
