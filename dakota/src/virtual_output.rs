/// Virtual Output Surface
///
/// This represents a virtual area that defines the region where a
/// Scene can be layed out. Some or all of it will be presented
/// using an Output.
// Austin Shafer - 2024
use crate::event::PlatformEventSystem;
use crate::{OutputId, PlatformEvent, Scene};
use utils::{log, Result};

use std::ops::DerefMut;

/// Virtual Output Surface
///
/// This represents a virtual area that defines the region where a
/// Scene can be layed out. Some or all of it will be presented
/// using an Output.
pub struct VirtualOutput {
    /// Internal ID
    pub(crate) d_id: OutputId,
    /// per-VirtualOutput event queues
    d_platform_event_system: ll::Component<PlatformEventSystem>,
    /// This is the current size of this virtual surface.
    /// This needs to be updated by the app.
    d_size: (u32, u32),
    /// Cached mouse position
    ///
    /// Mouse updates are relative, so we need to add them to the last
    /// known mouse location. That is the value stored here.
    d_mouse_pos: (i32, i32),
}

impl VirtualOutput {
    /// Create a new VirtualOutput
    ///
    /// This still needs to have its geometry assigned
    pub fn new(id: OutputId, evsys: ll::Component<PlatformEventSystem>) -> Result<Self> {
        evsys.set(&id, PlatformEventSystem::new());

        Ok(Self {
            d_id: id,
            d_size: (0, 0),
            d_mouse_pos: (0, 0),
            d_platform_event_system: evsys,
        })
    }

    /// Get the size of this virtual surface
    pub fn get_size(&self) -> (u32, u32) {
        self.d_size
    }

    /// Set the size of this virtual surface
    pub fn set_size(&mut self, size: (u32, u32)) {
        self.d_size = size;
    }

    /// Get the next currently unhandled event
    ///
    /// The app should do this in its main loop after dispatching.
    pub fn pop_event(&mut self) -> Option<PlatformEvent> {
        self.d_platform_event_system
            .get_mut(&self.d_id)
            .unwrap()
            .deref_mut()
            .pop_event()
    }

    /// Handle dakota-only events coming from the event system
    ///
    /// Most notably this handles scrolling
    pub fn handle_scrolling(
        &mut self,
        scene: &mut Scene,
        position: (i32, i32),
        relative_scroll: (i32, i32),
    ) -> Result<()> {
        // Update our mouse
        self.d_mouse_pos = position;

        {
            // Find viewport at this location
            let node = scene.get_viewport_at_position(self.d_mouse_pos.0, self.d_mouse_pos.1);
            let mut viewport = scene.d_viewports.get_mut(&node).unwrap();
            log::error!("original_scroll_offset: {:?}", viewport.scroll_offset);

            viewport.update_scroll_amount(relative_scroll.0, relative_scroll.1);
            log::error!("new_scroll_offset: {:?}", viewport.scroll_offset);
        }

        Ok(())
    }
}
