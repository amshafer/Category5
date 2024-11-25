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
use crate::event::EventSystem;
use crate::platform::OutputPlatform;
use crate::{dom, DakotaError, Event, Scene};
use utils::log;
use utils::{anyhow, timing::StopWatch, Error, Result};

pub struct Output {
    /// Our thundr output object
    pub(crate) d_display: th::Display,
    /// Platform handling specific to this output
    d_output_plat: Box<dyn OutputPlatform>,
    /// Our Event Queue
    d_event_sys: EventSystem,
    /// Cached mouse position
    ///
    /// Mouse updates are relative, so we need to add them to the last
    /// known mouse location. That is the value stored here.
    pub d_mouse_pos: (i32, i32),
    /// This counts how many times we need to loop after an out of date
    /// event. This is useful for forcing full refresh rate on resize.
    d_ood_counter: usize,
}

impl Output {
    pub fn new(window_plat: Box<dyn OutputPlatform>, display: th::Display) -> Result<Self> {
        Ok(Self {
            d_output_plat: window_plat,
            d_display: display,
            d_event_sys: EventSystem::new(),
            d_mouse_pos: (0, 0),
            d_ood_counter: 30,
        })
    }

    /// Create a scene compatible with this Output
    ///
    /// Resources will be created on the GPU this Output is present on.
    pub fn create_scene(&self) -> Result<Scene> {
        Scene::new(
            self.d_display.d_dev.clone(),
            self.d_display.get_resolution(),
        )
    }

    /// Get the current size of the drawing region for this display
    pub fn get_resolution(&self) -> (u32, u32) {
        self.d_display.get_resolution()
    }

    /// Get the major, minor of the DRM device currently in use
    pub fn get_drm_dev(&self) -> (i64, i64) {
        self.d_display.get_drm_dev()
    }

    /// Set the resolution of the current window
    pub fn set_resolution(&mut self, scene: &mut Scene, width: u32, height: u32) -> Result<()> {
        let dom = scene
            .d_dom
            .as_ref()
            .ok_or(anyhow!("Only DOM objects can be refreshed"))?;
        self.d_output_plat
            .set_geometry(&dom.window, (width, height))?;

        Ok(())
    }

    /// Get the slice of currently unhandled events
    ///
    /// The app should do this in its main loop after dispatching.
    /// These will be cleared during each dispatch.
    pub fn drain_events<'b>(&'b mut self) -> std::collections::vec_deque::Drain<'b, Event> {
        self.d_event_sys.drain_events()
    }

    /// Handle vulkan swapchain out of date. This is probably because the
    /// window's size has changed. This will requery the window size and
    /// refresh the layout tree.
    fn handle_ood(&mut self, scene: &mut Scene) -> Result<()> {
        let new_res = self.d_display.get_resolution();
        let dom = scene
            .d_dom
            .as_ref()
            .ok_or(anyhow!("Only DOM objects can be refreshed"))?;

        self.d_event_sys.add_event_window_resized();
        self.d_event_sys.add_event_window_redraw_complete(
            dom,
            dom::Size {
                width: new_res.0,
                height: new_res.1,
            },
        );

        scene.d_needs_redraw = true;
        scene.d_needs_refresh = true;
        self.d_ood_counter = 30;
        scene.d_window_dims = new_res;
        Ok(())
    }

    /// Handle dakota-only events coming from the event system
    ///
    /// Most notably this handles scrolling
    fn handle_private_events(&mut self, scene: &mut Scene) -> Result<()> {
        for ev in self.d_event_sys.es_dakota_event_queue.drain(0..) {
            match ev {
                Event::InputScroll {
                    position,
                    xrel,
                    yrel,
                    ..
                } => {
                    let x = match *xrel {
                        Some(v) => v as i32,
                        None => 0,
                    };
                    let y = match *yrel {
                        Some(v) => v as i32,
                        None => 0,
                    };
                    // Update our mouse
                    self.d_mouse_pos = (position.0 as i32, position.1 as i32);

                    // Find viewport at this location
                    let node = scene.viewport_at_pos(self.d_mouse_pos.0, self.d_mouse_pos.1);
                    let mut viewport = scene.d_viewports.get_mut(&node).unwrap();
                    log::error!("original_scroll_offset: {:?}", viewport.scroll_offset);

                    viewport.update_scroll_amount(x, y);
                    log::error!("new_scroll_offset: {:?}", viewport.scroll_offset);

                    scene.d_needs_redraw = true;
                }
                // Ignore all other events for now
                _ => {}
            }
        }

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

    /// run the dakota thread.
    ///
    /// Dakota requires takover of one thread, because that's just how winit
    /// wants to work. It's annoying, but we live with it. `func` will get
    /// called before the next frame is drawn, it is the winsys event handler
    /// for the app.
    ///
    /// This will (under construction):
    /// * wait for new sdl events (blocking)
    /// * handle events (input, etc)
    /// * tell thundr to render if needed
    ///
    /// Returns true if we should terminate i.e. the window was closed.
    /// Timeout is in milliseconds, and is the timeout to wait for
    /// window system events.
    pub fn dispatch(&mut self, scene: &mut Scene, mut timeout: Option<usize>) -> Result<()> {
        let mut first_loop = true;

        loop {
            if !first_loop || self.d_ood_counter > 0 {
                timeout = Some(0);
                self.d_ood_counter -= 1;
                scene.d_needs_redraw = true;
            }
            first_loop = false;

            // First handle input and platform changes
            match self.dispatch_platform(scene, timeout) {
                Ok(()) => {}
                Err(e) => {
                    if e.downcast_ref::<DakotaError>() == Some(&DakotaError::OUT_OF_DATE) {
                        continue;
                    } else {
                        return Err(e);
                    }
                }
            };

            // Now render the frame
            match self.dispatch_rendering(scene) {
                Ok(()) => {}
                Err(e) => {
                    if e.downcast_ref::<DakotaError>() == Some(&DakotaError::OUT_OF_DATE) {
                        continue;
                    } else {
                        return Err(e);
                    }
                }
            };

            return Ok(());
        }
    }

    /// Dispatch platform specific handling code
    ///
    /// This will handle user input and other things like that. This function
    /// is internally called by the `dispatch` call and does not perform any
    /// drawing.
    pub fn dispatch_platform(&mut self, scene: &mut Scene, timeout: Option<usize>) -> Result<()> {
        // First run our window system code. This will check if wayland/X11
        // notified us of a resize, closure, or need to redraw
        let plat_res = self.d_plat.run(
            &mut self.d_event_sys,
            scene
                .d_dom
                .as_ref()
                .ok_or(anyhow!("Id passed to Dispatch must be a DOM object"))?,
            timeout,
        );

        match plat_res {
            Ok(needs_redraw) => {
                if needs_redraw {
                    scene.d_needs_redraw = needs_redraw
                }
            }
            Err(th::ThundrError::OUT_OF_DATE) => {
                // This is a weird one
                // So the above OUT_OF_DATEs are returned from thundr, where we
                // can expect it will handle OOD itself. But here we have
                // OUT_OF_DATE returned from our SDL2 backend, so we need
                // to tell Thundr to do OOD itself
                self.d_display.handle_ood()?;
                self.handle_ood(scene)?;
                return Err(th::ThundrError::OUT_OF_DATE.into());
            }
            Err(e) => return Err(Error::from(e).context("Thundr: presentation failed")),
        };

        return Ok(());
    }

    /// Draw the next frame
    ///
    /// This dispatches *only* the rendering backend of Dakota. The `dispatch_platform`
    /// call *must* take place before this in order for correct updates to happen, as
    /// this will only render the current state of Dakota.
    pub fn dispatch_rendering(&mut self, scene: &mut Scene) -> Result<()> {
        let mut stop = StopWatch::new();

        // Now handle events like scrolling before we calculate sizes
        self.handle_private_events(scene)?;

        if scene.needs_refresh() {
            let mut layout_stop = StopWatch::new();
            layout_stop.start();
            scene.refresh()?;
            layout_stop.end();
            log::debug!(
                "Dakota spent {} ms refreshing the layout",
                layout_stop.get_duration().as_millis()
            );
        }
        stop.start();

        // if needs redraw, then tell thundr to draw and present a frame
        // At every step of the way we check if the drawable has been resized
        // and will return that to the dakota user so they have a chance to resize
        // anything they want
        if scene.d_needs_redraw {
            match self.draw_surfacelists(scene) {
                Ok(()) => {}
                Err(th::ThundrError::OUT_OF_DATE) => {
                    self.handle_ood(scene)?;
                    return Err(th::ThundrError::OUT_OF_DATE.into());
                }
                Err(e) => return Err(Error::from(e).context("Thundr: drawing failed with error")),
            };
            scene.d_needs_redraw = false;

            // Notify the app that we just drew a frame and it should prepare the next one
            self.d_event_sys.add_event_window_redraw_complete(
                scene
                    .d_dom
                    .as_ref()
                    .ok_or(anyhow!("Id passed to Dispatch must be a DOM object"))?,
            );
            stop.end();
            log::debug!(
                "Dakota spent {} ms drawing this frame",
                stop.get_duration().as_millis()
            );
        }

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
