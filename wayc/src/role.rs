/// The role of a surface defines its use and how it is displayed.
pub enum Role {
    /// This surface is layered on top of another surface. It is used to
    /// tell the compositor to do the heavy lifting and composite the surfaces
    /// for us.
    SubSurface,
    /// This surface is a root window in a desktop environment. This type is
    /// backed by the xdg window management protocol.
    Desktop,
    /// No role has been assigned to this surface (so far). Signifies that
    /// the surface should do nothing.
    Unassigned,
}
