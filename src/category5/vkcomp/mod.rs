// vkcomp - The vulkan compositor
//
// Austin Shafer - 2020

// We need to create the tree of modules that make up
// the vulkan compositor:

// Window Manager: This provides a nice API for the
// upper layers to create/move/modify windows. It
// takes care of driving the Renderer
// Does not contain any vulkan or unsafe code.
pub mod wm;
mod release_info;
