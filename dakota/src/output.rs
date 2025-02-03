//! Dakota Output Representation
//!
//! An Output in Dakota is a renderable surface which contains a layout
//! tree of Elements. This Element tree controls the content presented
//! on this Output, which may take the form of a toplevel window, a
//! subsurface, or some other display. Each output is driven separately
//! but is identified by an OutputId which lets the main event loop
//! know which Output to dispatch.
// Austin Shafer - 2024
extern crate utils;
use crate::event::OutputEventSystem;
use crate::platform::OutputPlatform;
use crate::{OutputEvent, OutputId, Scene, VirtualOutput};
use utils::log;
use utils::{anyhow, Error, Result};

use std::ops::DerefMut;
use std::sync::{Arc, RwLock};

/// OutputInfo
///
/// This trait encapsulates per Output backend information about
/// a particular Display area. This info may describe an available
/// window system or a physical display device that an Output object
/// can be created on.
///
/// These OutputInfo objects are used to initialize a specific Output.
/// Some output types may support creation of multiple Output objects,
/// or may only support a single Output.
#[derive(Clone)]
pub struct OutputInfo {
    /// Our info payload.
    /// This is a swapchain backend specific set of information that
    /// holds the physical display information, surface objects, etc.
    pub(crate) oi_payload: Arc<dyn th::DisplayInfoPayload>,
    /// Cached event queue list. This will be used to add events to
    /// existing Outputs.
    oi_event_queues: ll::Component<OutputEventSystem>,
    oi_internal: Arc<RwLock<OutputInfoInternal>>,
}

/// OutputInfo is clonable because we need to both keep an internal
/// list of it and pass it in as an argument to create_output, which
/// requires interior mutability for tracking active usage counts.
pub(crate) struct OutputInfoInternal {
    /// Event Queues consuming this Output source.
    /// When this output source is destroyed we need to
    /// The length of this array is the number of active displays,
    /// which will be returned in the active count.
    oi_outputs: Vec<OutputId>,
    /// Has this output type been destroyed
    oi_destroyed: bool,
}

impl OutputInfo {
    /// Initialize an empty OutputInfo with the provided payload
    pub(crate) fn new(
        evsys: ll::Component<OutputEventSystem>,
        payload: Arc<dyn th::DisplayInfoPayload>,
    ) -> Self {
        Self {
            oi_payload: payload,
            oi_event_queues: evsys,
            oi_internal: Arc::new(RwLock::new(OutputInfoInternal {
                oi_outputs: Vec::with_capacity(1),
                oi_destroyed: false,
            })),
        }
    }

    /// Add this OutputId to our internal tracker
    pub(crate) fn add_output(&self, id: OutputId) {
        self.oi_internal.write().unwrap().oi_outputs.push(id)
    }

    /// Destroy this OutputInfo
    ///
    /// This signals that the output type this represents is no longer
    /// available. This will send the Destroyed event to all child Outputs.
    pub fn destroy(&self) {
        let mut internal = self.oi_internal.write().unwrap();
        assert!(!internal.oi_destroyed);
        internal.oi_destroyed = true;

        for id in internal.oi_outputs.iter() {
            self.oi_event_queues
                .get_mut(&id)
                .unwrap()
                .add_event_destroyed();
        }
    }

    /// Multiple Displays may be created for the platform this info describes
    /// or only one, depending on the capabilities of this Display backend.
    /// Returns the number of Displays we can create for this output.
    pub fn max_output_count(&self) -> usize {
        self.oi_payload.max_output_count()
    }

    /// Returns true if we can create another Output from this info.
    ///
    /// This will return false if the current maximum number of Outputs has
    /// been created, or if this OutputInfo has been destroyed.
    pub fn can_create_output(&self) -> bool {
        let internal = self.oi_internal.read().unwrap();

        !internal.oi_destroyed && internal.oi_outputs.len() < self.max_output_count()
    }
}

/// Dakota Output
///
/// The Output object controls all presentation and rendering logic,
/// allowing a virtual scene to be redrawn and displayed on the screen.
/// An Output may be a toplevel desktop window, a subsurface, or
/// a physical display.
///
/// Outputs are the final line in the content pipeline. After a Scene
/// has been created and layout calculated on a VirtualOutput, we can
/// actually composit the scene and show it to the user on this Output.
pub struct Output {
    /// Internal ID
    pub(crate) d_id: OutputId,
    /// Our thundr output object
    pub(crate) d_display: th::Display,
    /// Platform handling specific to this output
    d_output_plat: Box<dyn OutputPlatform>,
    /// per-Output event queues
    d_output_event_system: ll::Component<OutputEventSystem>,
    /// Offset of this Output within the VirtualOutput. This controls
    /// which region of the Scene is drawn.
    pub(crate) d_offset: (i32, i32),
}

impl Output {
    pub fn new(
        window_plat: Box<dyn OutputPlatform>,
        display: th::Display,
        id: OutputId,
        evsys: ll::Component<OutputEventSystem>,
    ) -> Result<Self> {
        evsys.set(&id, OutputEventSystem::new());

        Ok(Self {
            d_id: id,
            d_output_event_system: evsys,
            d_output_plat: window_plat,
            d_display: display,
            d_offset: (0, 0),
        })
    }

    /// Create a scene compatible with this Output and VirtualOutput
    ///
    /// Resources will be created on the GPU this Output is present on.
    pub fn create_scene(&self, virtual_output: &VirtualOutput) -> Result<Scene> {
        Scene::new(self.d_display.d_dev.clone(), virtual_output.get_size())
    }

    /// Get the current size of the drawing region for this display
    pub fn get_resolution(&self) -> (u32, u32) {
        self.d_display.get_resolution()
    }

    /// Set the presentation offset of this Output
    ///
    /// Sets the offset of this Output within the VirtualOutput. This controls
    /// which region of the Scene is drawn.
    pub fn set_offset(&mut self, x: i32, y: i32) {
        self.d_offset = (x, y);
    }

    /// Get the major, minor of the DRM device currently in use
    pub fn get_drm_dev(&self) -> Option<(i64, i64)> {
        self.d_display.get_drm_dev()
    }

    /// Set the resolution of the current window
    pub fn set_resolution(&mut self, scene: &mut Scene, width: u32, height: u32) -> Result<()> {
        let dom = scene
            .d_dom
            .as_ref()
            .ok_or(anyhow!("No DOM object provided in Scene"))?;
        self.d_output_plat
            .set_geometry(&dom.window, (width, height))?;

        Ok(())
    }

    /// Get the slice of currently unhandled events
    ///
    /// The app should do this in its main loop after dispatching.
    /// These will be cleared during each dispatch.
    ///
    /// Returns None when there are no pending events available
    pub fn pop_event(&mut self) -> Option<OutputEvent> {
        self.d_output_event_system
            .get_mut(&self.d_id)
            .unwrap()
            .deref_mut()
            .pop_event()
    }

    /// Mark this Output as needing a redraw
    ///
    /// This should be called after a Scene this Output is presenting
    /// has been updated. This will add a `OutputEvent::Redraw` to this
    /// Output's event queue
    pub fn request_redraw(&mut self) {
        let mut evsys = self.d_output_event_system.get_mut(&self.d_id).unwrap();
        evsys.add_event_redraw();
    }

    /// Handle Output Resize
    ///
    /// This function should be called when the Resized event is received.
    ///
    /// Handle vulkan swapchain out of date. This is probably because the
    /// window's size has changed. This will requery the window size and
    /// refresh the layout tree.
    pub fn handle_resize(&mut self) -> Result<()> {
        self.d_display.handle_ood()?;

        self.request_redraw();

        Ok(())
    }

    /// Get the DRM format modifiers supported by this display
    pub fn get_supported_drm_render_modifiers(&self) -> Vec<u64> {
        self.d_display
            .d_dev
            .get_supported_drm_render_modifiers()
            .iter()
            .map(|m| m.drm_format_modifier)
            .collect()
    }

    /// Draw the next frame
    ///
    /// This dispatches *only* the rendering backend of Dakota. The `dispatch_platform`
    /// call *must* take place before this in order for correct updates to happen, as
    /// this will only render the current state of Dakota.
    pub fn redraw(&mut self, _virtual_output: &VirtualOutput, scene: &mut Scene) -> Result<()> {
        match self.draw_surfacelists(scene) {
            Ok(()) => {}
            Err(th::ThundrError::OUT_OF_DATE) => {
                // If Thundr returned out of date while
                self.d_output_event_system
                    .get_mut(&self.d_id)
                    .unwrap()
                    .deref_mut()
                    .add_event_resized();
                log::debug!("Dakota::Output: Swapchain out of date, triggering resize");
            }
            Err(e) => return Err(Error::from(e).context("Thundr: drawing failed with error")),
        };
        log::debug!("Dakota::Output: finished dispatching rendering",);

        return Ok(());
    }

    /// Dump the current swapchain image to a file
    ///
    /// This dumps the image contents to a simple PPM file, used for automated testing
    #[allow(dead_code)]
    pub fn dump_framebuffer(&mut self, filename: &str) -> th::MappedImage {
        self.d_display.dump_framebuffer(filename)
    }
}
