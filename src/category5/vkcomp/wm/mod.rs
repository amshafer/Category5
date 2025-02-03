//! # Vulkan Compositor
//!
//! Also known as the `vkcomp` subsystem.
//!
//! The naming of this subsystem is somewhat confusing. It is called a
//! compositor, but it seems to act as a window manager and doesn't
//! actually do any composition itself. Category5 is the compositor,
//! but `vkcomp` is the code that handles window composition. `vkcomp`
//! takes the current state of the `atmosphere` and draws the windows on
//! the user's display.
//!
//! `vkcomp` itself is just the behavior for generating draw commands and
//! handling window presentation. The window positions are determined by
//! `ways` and `input`, and the Vulkan rendering is handled by `dakota`.
//!
//! `thundr` exists because during development I observed that the type of
//! Vulkan renderer I was creating could be reused in other places. For
//! example, a UI toolkit might also want to composite multiple buffers or
//! panels together to make a user interface. `vkcomp` is just one such
//! usage of `thundr`.
//!
//! ### Code
//! * `wm` - The window manager
//!   * `wm` specifies how to draw a typical desktop. There are a
//!   collection of applications at various sizes/positions, a desktop
//!   background, toolbars, etc. It generates a list of surfaces which are
//!   handed to `thundr` to be drawn.
//! * `wm/tasks.rs` - A list of tasks that the atmosphere passes to
//! vkcomp. These are one-time events, and usually just tell `vkcomp` that
//! an object was created and it needs to allocate gpu resources.
//! * `release_info.rs` - Release info are structs that specify values to
//! drop after `vkcomp` is done using them. This is used to release
//! wl_buffers once they are no longer in use by the gpu.

// Austin Shafer - 2020

extern crate chrono;
extern crate dakota as dak;
extern crate image;
extern crate lluvia as ll;
extern crate utils;

use dak::{anyhow, dom, DakotaId};

use crate::category5::atmosphere::*;
use utils::{log, Context, Result};

pub mod task;
use task::*;

#[cfg(feature = "renderdoc")]
extern crate renderdoc;
#[cfg(feature = "renderdoc")]
use renderdoc::RenderDoc;

// Menu bar is 16 pixels tall
static MENUBAR_SIZE: i32 = 32;
pub static DESKTOP_OFFSET: i32 = MENUBAR_SIZE;

/// WindowManagerOutput
///
/// This represents a monitor we are displaying a desktop
/// on. This will present a region of the virtual output
/// surface and has its own menubar/widgets
pub struct WMOutput {
    /// This is our presentation object which actually shows pixels
    /// on a presentable surface such as a physical display.
    wm_output: dak::Output,
    /// This Element is the container that holds this output's
    /// background effects
    wm_background_root: DakotaId,
    /// This is the element on the effect layer for this Output's
    /// desktop.
    /// The menubar and others will be children of this.
    wm_effects_root: DakotaId,
    /// The date time string UI element.
    wm_datetime: DakotaId,
}

/// Encapsulates vkcomp and provides a sensible windowing API
///
/// This layer provides graphical operations to the above
/// layers. It will support two classes of displayed objs,
/// windows (has content and a titlebar) and sprites.
///
/// Sprites should only be used for desktop effects, such
/// as notifications. Sprites are not owned by a client
/// whereas windows are.
pub struct WindowManager {
    wm_atmos_ids: Vec<SurfaceId>,
    /// The list of displays we are presenting desktops to
    wm_outputs: Vec<WMOutput>,
    /// The root element for our scene
    ///
    /// In Dakota layout is heirarchical, so we have a root node that we attach all
    /// elements to. This is that base node.
    #[allow(dead_code)]
    wm_scene_root: DakotaId,
    /// The background decorations
    ///
    /// This layer holds things like the desktop background or any desktop decorations
    /// that are composited behind the applications
    wm_background_layer: DakotaId,
    /// The window area for this desktop
    ///
    /// This is a Dakota element that represents the region where all client windows
    /// are laid out. This is layered behind the WMOutput's element.
    wm_app_layer: DakotaId,
    /// This is the effects layer, which holds the per-Output window or menu bars.
    /// This is layed out on top of the desktop layer.
    wm_effects_layer: DakotaId,
    /// Font definition for UI widgets
    wm_menubar_font: DakotaId,
    /// The cursor layer. This is the uppermost layer which lays the cursor
    /// image on top of everything. This is the size of the virtual output and
    /// the cursor element moves on top of it.
    #[allow(dead_code)]
    wm_cursor_layer: DakotaId,
    /// Image representing the software cursor
    wm_cursor: Option<DakotaId>,
    /// Category5's cursor, used when the client hasn't set one.
    wm_default_cursor: DakotaId,
    #[cfg(feature = "renderdoc")]
    wm_renderdoc: RenderDoc<renderdoc::V141>,
}

impl WindowManager {
    /// Returns an ID for an element bound with a defaul texture resource
    fn get_default_cursor(scene: &mut dak::Scene) -> DakotaId {
        let image = scene.create_resource().unwrap();
        scene
            .define_resource_from_image(
                &image,
                std::path::Path::new("images/cursor.png"),
                dom::Format::ARGB8888,
            )
            .expect("Could not import background image into scene");
        let surf = scene.create_element().unwrap();
        scene.offset().set(
            &surf,
            dom::RelativeOffset {
                x: dom::Value::Constant(0),
                y: dom::Value::Constant(0),
            },
        );
        scene.width().set(&surf, dom::Value::Constant(10));
        scene.height().set(&surf, dom::Value::Constant(15));
        scene.resource().set(&surf, image.clone());

        surf
    }

    /// Define all of the Dakota elements that make up the menu bar
    /// at the top of the screen
    fn create_menubar(scene: &mut dak::Scene, menubar_font: DakotaId) -> DakotaId {
        let barcolor = scene.create_resource().unwrap();
        scene
            .resource_color()
            .set(&barcolor, dak::dom::Color::new(0.085, 0.09, 0.088, 0.9));

        let menubar = scene.create_element().unwrap();
        // Make our bar 16 px tall but stretch across the screen
        scene.width().set(&menubar, dom::Value::Relative(1.0));
        scene
            .height()
            .set(&menubar, dom::Value::Constant(MENUBAR_SIZE));
        scene.resource().set(&menubar, barcolor);

        let name = scene.create_element().unwrap();
        scene.set_text_regular(&name, "Category5");
        scene.text_font().set(&name, menubar_font);
        scene.add_child_to_element(&menubar, name);

        return menubar;
    }

    /// Refresh the date and time string in the menubar
    ///
    /// This should be called every time change.
    pub fn refresh_datetime(&mut self, scene: &mut dak::Scene) {
        let date = chrono::Local::now();

        for wmo in self.wm_outputs.iter() {
            // https://docs.rs/chrono-wasi07/latest/chrono/format/strftime/index.html
            scene.set_text_regular(
                &wmo.wm_datetime,
                &date.format("%a %B %e %l:%M %p").to_string(),
            );
            scene
                .text_font()
                .set(&wmo.wm_datetime, self.wm_menubar_font.clone());
        }

        log::error!(
            "Updated time to: {}",
            date.format("%a %B %e %l:%M %p").to_string()
        );
    }

    /// Reorganize our Outputs within the global VirtualOutput
    ///
    /// This is called after we have added or removed an output and
    /// we need to recalculate the positions of the remainders. It will
    /// set the Output positions and then calculate the new total VirtualOutput
    /// size.
    pub fn reposition_outputs(
        &mut self,
        virtual_output: &mut dak::VirtualOutput,
        scene: &mut dak::Scene,
    ) -> Result<()> {
        let mut next_offset_x = 0;
        let mut new_extent = (0, 0);

        // Position this new output as the rightmost.
        // We do this by summing up the widths of all outputs so far
        for output in self.wm_outputs.iter_mut() {
            let output_size = output.wm_output.get_resolution();

            // For now just align them all at the top
            output.wm_output.set_offset(next_offset_x, 0);
            scene.offset().set(
                &output.wm_background_root,
                dom::RelativeOffset::new(
                    dom::Value::Constant(next_offset_x),
                    dom::Value::Constant(0),
                ),
            );
            scene.offset().set(
                &output.wm_effects_root,
                dom::RelativeOffset::new(
                    dom::Value::Constant(next_offset_x),
                    dom::Value::Constant(0),
                ),
            );

            // Ensure the size of the output's root elements matches
            // the size of the Output itself.
            let resolution = output.wm_output.get_resolution();
            scene.width().set(
                &output.wm_background_root,
                dom::Value::Constant(resolution.0 as i32),
            );
            scene.height().set(
                &output.wm_background_root,
                dom::Value::Constant(resolution.1 as i32),
            );
            scene.width().set(
                &output.wm_effects_root,
                dom::Value::Constant(resolution.0 as i32),
            );
            scene.height().set(
                &output.wm_effects_root,
                dom::Value::Constant(resolution.1 as i32),
            );

            next_offset_x += output_size.0 as i32;
            new_extent.0 += output_size.0;
            new_extent.1 += output_size.1;
        }

        // Resize our scene to accommodate it
        virtual_output.set_size(new_extent);

        Ok(())
    }

    /// Add a new output to the system
    ///
    /// This adds the Output to our list and resizes the VirtualOutput
    /// accordingly.
    pub fn add_output(
        &mut self,
        dakota: &mut dak::Dakota,
        output_info: &dak::OutputInfo,
        virtual_output: &mut dak::VirtualOutput,
        scene: &mut dak::Scene,
    ) -> Result<()> {
        let output = dakota
            .create_output_with_info(output_info, virtual_output)
            .context("Failed to create Dakota Output")?;

        // First create the root element for this output
        // ------------------------------------------------------------------
        let background_root = scene.create_element().unwrap();
        scene.add_child_to_element(&self.wm_background_layer, background_root.clone());
        // set the background for this desktop
        let image = scene.create_resource().unwrap();
        scene
            .define_resource_from_image(
                &image,
                std::path::Path::new("images/cat5_desktop.png"),
                dom::Format::ARGB8888,
            )
            .expect("Could not import background image into scene");
        scene.resource().set(&background_root, image);

        let effects_root = scene.create_element().unwrap();
        scene.add_child_to_element(&self.wm_effects_layer, effects_root.clone());

        // Now create our menu bar across the top of the screen
        // ------------------------------------------------------------------
        let menubar = Self::create_menubar(scene, self.wm_menubar_font.clone());
        scene.add_child_to_element(&effects_root, menubar.clone());

        // Add a text region to display our date and time
        // ------------------------------------------------------------------
        let datetime = scene.create_element().unwrap();
        scene.height().set(&datetime, dom::Value::Relative(1.0));
        scene.content().set(
            &menubar,
            dom::Content {
                el: datetime.clone(),
            },
        );

        // Now we can create our output tracking object and add it to our
        // output list
        self.wm_outputs.push(WMOutput {
            wm_output: output,
            wm_datetime: datetime,
            wm_background_root: background_root,
            wm_effects_root: effects_root,
        });

        Ok(())
    }

    /// Create a new WindowManager
    ///
    /// This will create all the graphical resources needed for
    /// the compositor. The WindowManager will create and own
    /// the Thundr, thereby readying the display to draw.
    pub fn new(
        dakota: &mut dak::Dakota,
        virtual_output: &mut dak::VirtualOutput,
        scene: &mut dak::Scene,
        atmos: &mut Atmosphere,
    ) -> Result<WindowManager> {
        #[cfg(feature = "renderdoc")]
        let doc = RenderDoc::new().unwrap();

        // Tell the atmosphere rend's resolution
        let res = virtual_output.get_size();
        atmos.set_resolution(res);

        // Create a DOM object that all others will hang off of
        // ------------------------------------------------------------------
        let root = scene.create_element().unwrap();
        // Manually set the size to the parent container so that its size
        // isn't derived from the image we set as the desktop background
        scene.width().set(&root, dom::Value::Relative(1.0));
        scene.height().set(&root, dom::Value::Relative(1.0));

        scene.set_dakota_dom(dak::dom::DakotaDOM {
            version: "0.0.1".to_string(),
            window: dak::dom::Window {
                title: "Category5".to_string(),
                size: Some(res),
                events: dak::dom::WindowEvents {
                    resize: None,
                    redraw_complete: None,
                    closed: None,
                },
            },
            root_element: root.clone(),
        });

        let menubar_font = scene.create_font().unwrap();
        scene.define_font(
            &menubar_font,
            dom::Font {
                name: "Menubar".to_string(),
                font_name: "JetBrainsMono".to_string(),
                pixel_size: 16,
                color: Some(dom::Color {
                    r: 0.941,
                    g: 0.921,
                    b: 0.807,
                    a: 1.0,
                }),
            },
        );

        // Next add a dummy element to place all of the background desktop decorations on
        // ------------------------------------------------------------------
        let background_layer = scene.create_element().unwrap();
        scene.add_child_to_element(&root, background_layer.clone());
        scene.offset().set(
            &background_layer,
            dom::RelativeOffset::new(dom::Value::Constant(0), dom::Value::Constant(0)),
        );
        scene
            .width()
            .set(&background_layer, dom::Value::Relative(1.0));
        scene
            .height()
            .set(&background_layer, dom::Value::Relative(1.0));

        // Next add a dummy element to place all of the client window child elements
        // inside of.
        // ------------------------------------------------------------------
        let app_layer = scene.create_element().unwrap();
        scene.add_child_to_element(&root, app_layer.clone());
        scene.offset().set(
            &app_layer,
            dom::RelativeOffset::new(dom::Value::Constant(0), dom::Value::Constant(0)),
        );
        scene.width().set(&app_layer, dom::Value::Relative(1.0));
        scene.height().set(&app_layer, dom::Value::Relative(1.0));

        // Above this lies the app_layer effects. These are layed out
        // above the toplevel windows.
        // ------------------------------------------------------------------
        let effects_layer = scene.create_element().unwrap();
        scene.offset().set(
            &effects_layer,
            dom::RelativeOffset::new(dom::Value::Constant(0), dom::Value::Constant(0)),
        );
        scene.add_child_to_element(&root, effects_layer.clone());
        scene.width().set(&effects_layer, dom::Value::Relative(1.0));
        scene
            .height()
            .set(&effects_layer, dom::Value::Relative(1.0));

        // now add a cursor on top of this
        // ------------------------------------------------------------------
        let cursor_layer = scene.create_element().unwrap();
        scene.offset().set(
            &cursor_layer,
            dom::RelativeOffset::new(dom::Value::Constant(0), dom::Value::Constant(0)),
        );
        scene.add_child_to_element(&root, cursor_layer.clone());
        scene.width().set(&cursor_layer, dom::Value::Relative(1.0));
        scene.height().set(&cursor_layer, dom::Value::Relative(1.0));

        let cursor = WindowManager::get_default_cursor(scene);
        scene.add_child_to_element(&cursor_layer, cursor.clone());

        let mut ret = WindowManager {
            wm_outputs: Vec::with_capacity(1),
            wm_scene_root: root,
            wm_app_layer: app_layer,
            wm_background_layer: background_layer,
            wm_effects_layer: effects_layer,
            wm_cursor_layer: cursor_layer,
            wm_cursor: Some(cursor.clone()),
            wm_default_cursor: cursor,
            wm_menubar_font: menubar_font,
            wm_atmos_ids: Vec::new(),
            #[cfg(feature = "renderdoc")]
            wm_renderdoc: doc,
        };

        // Create an Output for each display we have detected
        let infos = dakota.get_output_info();
        for info in dakota.get_output_info().iter() {
            ret.add_output(dakota, info, virtual_output, scene)
                .context("Creating output from info")?;
        }

        // Allow for overriding the display count. This is espescially
        // useful for forcing multiple headless or desktop windows for
        // testing multiple outputs.
        // This env var only takes effect on Output types that support
        // creation of multiple Outputs
        if let Ok(value) = std::env::var("CATEGORY5_FORCE_DISPLAY_COUNT") {
            let count = value
                .parse::<usize>()
                .expect("CATEGORY5_FORCE_DISPLAY_COUNT must be an integer");

            if infos.len() == 1 && infos[0].max_output_count() > count {
                for _ in infos.len()..count {
                    ret.add_output(dakota, &infos[0], virtual_output, scene)
                        .context("Creating output from info")?;
                }
            }
        }

        ret.reposition_outputs(virtual_output, scene)
            .context("Failed to position Outputs")?;

        if ret.wm_outputs.len() == 0 {
            return Err(anyhow!("Could not find an Output to present to"));
        }

        if let Some(drm_dev) = ret.wm_outputs[0].wm_output.get_drm_dev() {
            atmos.set_drm_dev(drm_dev);
        }

        ret.refresh_datetime(scene);

        return Ok(ret);
    }

    /// Set the desktop background for the renderer
    ///
    /// This basically just creates a image with the max
    /// depth that takes up the entire screen.
    #[allow(dead_code)]
    fn set_background_from_mem(
        scene: &mut dak::Scene,
        elem: &DakotaId,
        texture: &[u8],
        tex_width: u32,
        tex_height: u32,
    ) {
        let image = scene.create_resource().unwrap();
        scene
            .define_resource_from_bits(
                &image,
                texture,
                tex_width,
                tex_height,
                0,
                dak::dom::Format::ARGB8888,
            )
            .unwrap();
        scene.resource().set(elem, image);
    }

    /// Flag this window to be killed.
    ///
    /// This adds it to our death list, which will be reaped next frame after
    /// we are done using its resources.
    fn close_window(
        &mut self,
        atmos: &mut Atmosphere,
        scene: &mut dak::Scene,
        id: &SurfaceId,
    ) -> Result<()> {
        log::debug!("Closing window {:?}", id);

        // remove this surface in case it is a toplevel window
        scene.remove_child_from_element(&self.wm_app_layer, id)?;
        // If this is a subsurface, remove it from its parent
        if let Some(parent) = atmos.a_parent_window.get_clone(id) {
            scene.remove_child_from_element(&parent, id)?;
        }

        Ok(())
    }

    /// Move the window to the front of the scene
    ///
    /// There is really only one toplevel window movement
    /// event: moving something to the top of the window stack
    /// when the user clicks on it and puts it into focus.
    fn move_to_front(
        &mut self,
        atmos: &mut Atmosphere,
        scene: &mut dak::Scene,
        win: &SurfaceId,
    ) -> Result<()> {
        // get and use the root window for this subsurface
        // in case it is a subsurface.
        let root = match atmos.a_root_window.get_clone(win) {
            Some(parent) => parent,
            None => win.clone(),
        };

        // Move this surface to the front child of the window parent
        scene
            .move_child_to_front(&self.wm_app_layer, &root)
            .context(format!("Moving window {:?} to the front", win))?;

        Ok(())
    }

    /// Add a new toplevel surface
    ///
    /// This maps a new toplevel surface and places it in the desktop. This
    /// is where the scene element is added to the desktop as a child.
    fn new_toplevel(&mut self, scene: &mut dak::Scene, surf: &SurfaceId) -> Result<()> {
        // We might have not added this element to the desktop, moving to front
        // as part of focus is one of the first things that happens when a
        // new window is created
        scene.add_child_to_element(&self.wm_app_layer, surf.clone());

        Ok(())
    }

    /// Update the current cursor image
    ///
    /// Wayland clients may assign a surface to serve as the cursor image.
    /// Here we update the current cursor.
    fn set_cursor(
        &mut self,
        atmos: &mut Atmosphere,
        scene: &mut dak::Scene,
        surf: Option<SurfaceId>,
    ) -> Result<()> {
        if let Some(old) = self.wm_cursor.as_ref() {
            scene.remove_child_from_element(&self.wm_cursor_layer, old)?;
            // Don't reset the cursor hotspot here. It's already been updated
            // by the wl_pointer handlers.
        }

        // Clear the cursor if the client unset it. Otherwise get the
        // new surface, add it as a child and set it.
        self.wm_cursor = surf;

        if let Some(surf) = self.wm_cursor.as_ref() {
            scene.add_child_to_element(&self.wm_cursor_layer, surf.clone());
            // Set the size of the cursor
            let surface_size = atmos.a_surface_size.get(surf).unwrap();
            scene
                .width()
                .set(surf, dom::Value::Constant(surface_size.0 as i32));
            scene
                .height()
                .set(surf, dom::Value::Constant(surface_size.1 as i32));
            log::debug!("Setting cursor image with size {:?}", *surface_size,);
        }

        Ok(())
    }

    /// Reset the cursor to the default.
    ///
    /// Used when we are no longer listening to the client's suggested
    /// cursor
    fn reset_cursor(&mut self, atmos: &mut Atmosphere, scene: &mut dak::Scene) -> Result<()> {
        if let Some(old) = self.wm_cursor.as_ref() {
            scene.remove_child_from_element(&self.wm_cursor_layer, old)?;
        }

        scene.add_child_to_element(&self.wm_cursor_layer, self.wm_default_cursor.clone());
        self.wm_cursor = Some(self.wm_default_cursor.clone());
        atmos.set_cursor_hotspot((0, 0));
        atmos.set_cursor_surface(None);

        Ok(())
    }

    /// Adds a new subsurface to the parent.
    ///
    /// The new subsurface will be moved to the top of the subsurface
    /// stack, as this is the default. The position may later be changed
    /// through the wl_subsurface interface.
    fn new_subsurface(
        &mut self,
        scene: &mut dak::Scene,
        surf: &SurfaceId,
        parent: &SurfaceId,
    ) -> Result<()> {
        scene.add_child_to_element(parent, surf.clone());
        // Under normal operation Dakota elements are restricted to the size of
        // their parent. We do not want this for XDG popup surfaces, which are
        // layered ontop of toplevel windows but are not restricted by their
        // bounds. This special attribute tells scene not to clip them.
        scene.unbounded_subsurface().set(surf, true);
        Ok(())
    }

    /// Look up and place a surface above another in the subsurface list
    ///
    /// win will be placed above other. The parent will be looked up by
    /// searching the root window from atmos.
    fn subsurf_place_above(
        &mut self,
        atmos: &mut Atmosphere,
        scene: &mut dak::Scene,
        win: &SurfaceId,
        other: &SurfaceId,
    ) -> Result<()> {
        self.subsurf_reorder_common(atmos, scene, dak::SubsurfaceOrder::Above, win, other)
    }

    /// Same as above, but place the subsurface below other.
    fn subsurf_place_below(
        &mut self,
        atmos: &mut Atmosphere,
        scene: &mut dak::Scene,
        win: &SurfaceId,
        other: &SurfaceId,
    ) -> Result<()> {
        self.subsurf_reorder_common(atmos, scene, dak::SubsurfaceOrder::Below, win, other)
    }

    fn subsurf_reorder_common(
        &mut self,
        atmos: &mut Atmosphere,
        scene: &mut dak::Scene,
        order: dak::SubsurfaceOrder,
        surf: &SurfaceId,
        other: &SurfaceId,
    ) -> Result<()> {
        let root = atmos
            .a_root_window
            .get_clone(surf)
            .expect("The window should have a root since it is a subsurface");

        scene.reorder_children_element(&root, order, surf, other)?;

        Ok(())
    }

    /// Dispatch window management tasks
    ///
    /// This is where we handle things like surface/element creation, window creation and
    /// destruction, etc.
    pub fn process_task(&mut self, atmos: &mut Atmosphere, scene: &mut dak::Scene, task: &Task) {
        log::debug!("wm: got task {:?}", task);
        let err = match task {
            Task::move_to_front(id) => self
                .move_to_front(atmos, scene, id)
                .context("Task: Moving window to front"),
            Task::new_subsurface { id, parent } => self
                .new_subsurface(scene, id, parent)
                .context("Task: new_subsurface"),
            Task::place_subsurface_above { id, other } => self
                .subsurf_place_above(atmos, scene, id, other)
                .context("Task: place_subsurface_above"),
            Task::place_subsurface_below { id, other } => self
                .subsurf_place_below(atmos, scene, id, other)
                .context("Task: place_subsurface_below"),
            Task::close_window(id) => self
                .close_window(atmos, scene, id)
                .context("Task: close_window"),
            Task::new_toplevel(id) => self.new_toplevel(scene, id).context("Task: new_toplevel"),
            Task::set_cursor { id } => self
                .set_cursor(atmos, scene, id.clone())
                .context("Task: set_cursor"),
            Task::reset_cursor => self
                .reset_cursor(atmos, scene)
                .context("Task: reset_cursor"),
        };

        match err {
            Ok(()) => {}
            Err(e) => log::error!("vkcomp: Task handler had error: {:?}", e),
        }
    }

    /// Record all the drawing operations for the current scene
    ///
    /// Vulkan requires that we record a list of operations into a command
    /// buffer which is later submitted for display. This method organizes
    /// the recording of draw operations for all elements in the desktop.
    ///
    /// params: a private info structure for the Thundr. It holds all
    /// the data about what we are recording.
    fn record_draw(&mut self, atmos: &mut Atmosphere, scene: &mut dak::Scene) {
        // get the latest cursor position
        // ----------------------------------------------------------------
        let (cursor_x, cursor_y) = atmos.get_cursor_pos();
        let hotspot = atmos.get_cursor_hotspot();
        log::debug!(
            "Drawing cursor at ({}, {}), with hotspot {:?}",
            cursor_x,
            cursor_y,
            hotspot
        );
        if let Some(cursor) = self.wm_cursor.as_mut() {
            scene.offset().set(
                &cursor,
                dom::RelativeOffset {
                    x: dom::Value::Constant((cursor_x as i32).saturating_sub(hotspot.0)),
                    y: dom::Value::Constant((cursor_y as i32).saturating_sub(hotspot.1)),
                },
            );
        }
        // ----------------------------------------------------------------

        // Draw all of our windows on the desktop
        // Each app should have one or more windows,
        // all of which we need to draw.
        // ----------------------------------------------------------------
        self.wm_atmos_ids.clear();
        // Cache the inorder surface list from atmos
        // This helps us avoid nasty borrow checker stuff by avoiding recursion
        // ----------------------------------------------------------------
        let aids = &mut self.wm_atmos_ids;
        atmos.map_inorder_on_surfs(|id, _| {
            aids.push(id);
            return true;
        });

        // do the draw call separately due to the borrow checker
        // throwing a fit if it is in the loop above.
        //
        // This section really just updates the size and position of all the
        // surfaces. They should already have images attached, and damage will
        // be calculated from the result.
        // ----------------------------------------------------------------
        for id in self.wm_atmos_ids.iter() {
            // Now render the windows
            // get parameters
            // ----------------------------------------------------------------
            let surface_pos = *atmos.a_surface_pos.get(id).unwrap();
            let surface_size = *atmos.a_surface_size.get(id).unwrap();
            log::debug!(
                "placing scene element at {:?} with size {:?}",
                surface_pos,
                surface_size
            );

            // update the th::Surface pos and size
            scene.offset().set(
                id,
                dom::RelativeOffset {
                    x: dom::Value::Constant(surface_pos.0 as i32),
                    y: dom::Value::Constant(surface_pos.1 as i32),
                },
            );
            scene
                .width()
                .set(id, dom::Value::Constant(surface_size.0 as i32));
            scene
                .height()
                .set(id, dom::Value::Constant(surface_size.1 as i32));
            // ----------------------------------------------------------------

            // Send any pending frame callbacks
            atmos.send_frame_callbacks_for_surf(id);
        }
    }

    /// The main event loop of the vkcomp thread
    pub fn render_frame(
        &mut self,
        virtual_output: &dak::VirtualOutput,
        scene: &mut dak::Scene,
        atmos: &mut Atmosphere,
    ) -> Result<()> {
        #[cfg(feature = "renderdoc")]
        if atmos.get_renderdoc_recording() {
            self.wm_renderdoc
                .start_frame_capture(std::ptr::null(), std::ptr::null());
        }

        // iterate through all the tasks that ways left
        // us in this hemisphere
        //  (aka process the work queue)
        while let Some(task) = atmos.get_next_wm_task() {
            self.process_task(atmos, scene, &task);
        }

        // If nothing has changed then we can exit
        //
        // TODO: track this per-output to prevent excess redraws
        if !atmos.is_changed() {
            return Ok(());
        }

        // start recording how much time we spent doing graphics
        log::debug!("_____________________________ FRAME BEGIN");

        // Update our dakota element positions
        self.record_draw(atmos, scene);
        scene
            .recompile(&virtual_output)
            .expect("Failed to recalculate layout");

        // Have Dakota redraw the scene.
        // For now do this for all outputs
        // TODO: per-output draw tracking
        for output in self.wm_outputs.iter_mut() {
            output
                .wm_output
                .redraw(virtual_output, scene)
                .context("Redrawing WM Output")?;
        }

        atmos.clear_changed();
        log::debug!("_____________________________ FRAME END");

        atmos.print_surface_tree();

        #[cfg(feature = "renderdoc")]
        if atmos.get_renderdoc_recording() {
            self.wm_renderdoc
                .end_frame_capture(std::ptr::null(), std::ptr::null());
        }

        Ok(())
    }

    /// Dispatch all Output handling
    ///
    /// This causes the WindowManager to run through the Outputs it has created
    /// and handle any events posted on them.
    pub fn dispatch_drawing(
        &mut self,
        virtual_output: &mut dak::VirtualOutput,
        scene: &mut dak::Scene,
        atmos: &mut Atmosphere,
    ) -> Result<()> {
        // If our state database was updated by input or wayland processing then
        // we need to rerender
        let mut needs_render = atmos.is_changed();

        //'outer: for output in self.wm_outputs.iter_mut() {
        'outer: for i in 0..self.wm_outputs.len() {
            while let Some(ev) = self.wm_outputs[i].wm_output.pop_event() {
                match &ev {
                    // Redraw our scene
                    dak::OutputEvent::Redraw => {
                        needs_render = true;
                    }
                    // Our output surface is out of date, reallocate it
                    dak::OutputEvent::Resized => {
                        let res = self.wm_outputs[i].wm_output.get_resolution();
                        {
                            atmos.mark_changed();
                            atmos.set_resolution(res);
                        }
                        // TODO: implement after refactoring WindowManager on top
                        //self.em_climate.send_all_geometry();

                        // First handle the resize on this output
                        self.wm_outputs[i]
                            .wm_output
                            .handle_resize()
                            .expect("Failed to resize output");

                        // Recalculate Output layout
                        self.reposition_outputs(virtual_output, scene)
                            .context("Failed to position Outputs")?;

                        // Break here to avoid redrawing before we have recalculated
                        // the Output positions
                        break 'outer;
                    }
                    dak::OutputEvent::Destroyed => {
                        // Remove our background and desktop elements
                        scene.remove_child_from_element(
                            &self.wm_background_layer,
                            &self.wm_outputs[i].wm_background_root,
                        )?;
                        scene.remove_child_from_element(
                            &self.wm_effects_layer,
                            &self.wm_outputs[i].wm_effects_root,
                        )?;

                        // Remove this output and reposition the remainders
                        self.wm_outputs.remove(i);
                        self.reposition_outputs(virtual_output, scene)
                            .context("Failed to position Outputs")?;

                        needs_render = true;

                        break 'outer;
                    }
                }
            }
        }

        if needs_render {
            self.render_frame(virtual_output, scene, atmos)?;
        }

        Ok(())
    }
}
