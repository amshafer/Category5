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

#![allow(dead_code)]
extern crate chrono;
extern crate dakota as dak;
extern crate image;
extern crate utils;

use dak::dom;
use dak::DakotaId;

use crate::category5::atmosphere::property_list::PropertyList;
use crate::category5::atmosphere::*;

pub use crate::utils::{anyhow, Result};
use utils::{log, timing::*, *};

pub mod task;
use super::release_info::DmabufReleaseInfo;
use task::*;

#[cfg(feature = "renderdoc")]
extern crate renderdoc;
#[cfg(feature = "renderdoc")]
use renderdoc::RenderDoc;

// Menu bar is 16 pixels tall
static MENUBAR_SIZE: i32 = 32;
pub static DESKTOP_OFFSET: i32 = MENUBAR_SIZE;

/// This consolidates the multiple resources needed
/// to represent a titlebar
struct Titlebar {
    /// The thick bar itself
    bar: DakotaId,
    /// One dot to rule them all. Used for buttons
    dot: DakotaId,
}

/// This represents a client window.
///
/// All drawn components are tracked with Image, this struct
/// keeps track of the window components (content imagees and
/// titlebar image).
#[derive(Clone)]
pub struct App {
    /// This id uniquely identifies the App
    a_id: WindowId,
    /// Because the images for imagees are used for both
    /// buffers in a double buffer system, when an App is
    /// deleted we need to avoid recording it in the next
    /// frame's cbuf.
    ///
    /// When this flag is set, the we will not be recorded
    /// and will instead be destroyed
    a_marked_for_death: bool,
    /// This is the set of geometric objects in the application
    a_surf: DakotaId,
    /// The image attached to `a_surf`
    a_image: Option<DakotaId>,
    /// Any server side decorations for this app.
    /// Right now this is (dot, bar)
    a_ssd: Option<(DakotaId, DakotaId)>,
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
    wm_atmos_ids: Vec<WindowId>,
    /// The root element for our scene
    ///
    /// In Dakota layout is heirarchical, so we have a root node that we attach all
    /// elements to. This is that base node.
    wm_dakota_root: DakotaId,
    /// Dakota DOM, the top level dakota object
    pub wm_dakota_dom: DakotaId,
    /// The menu bar across the top of the screen
    ///
    /// This is a Dakota element that holds all of the menu items and widgets
    /// in the top screen bar.
    wm_menubar: DakotaId,
    /// Font definition for UI widgets
    wm_menubar_font: DakotaId,
    /// The date time string UI element.
    wm_datetime: DakotaId,
    /// The window area for this desktop
    ///
    /// This is a Dakota element that represents the region where all client windows
    /// are laid out.
    wm_desktop: DakotaId,
    /// These are the surfaces that have been removed, and need their resources
    /// torn down. We keep this in a separate array so that we don't have to
    /// rescan the entire surface list every time we check for dead windows.
    wm_will_die: Vec<WindowId>,
    /// This is the set of applications in this scene
    wm_apps: PropertyList<App>,
    /// Image representing the software cursor
    wm_cursor: Option<DakotaId>,
    /// The offset of the cursor image
    wm_cursor_hotspot: (i32, i32),
    /// Category5's cursor, used when the client hasn't set one.
    wm_default_cursor: DakotaId,
    /// Title bar to draw above the windows
    wm_titlebar: Titlebar,
    #[cfg(feature = "renderdoc")]
    wm_renderdoc: RenderDoc<renderdoc::V141>,
}

impl WindowManager {
    /// Create a Titlebar resource
    ///
    /// The Titlebar will hold all of the components which make
    /// up all of the titlebars in a scene. These imagees will
    /// be colored differently when multidrawn
    fn get_default_titlebar(dakota: &mut dak::Dakota) -> Titlebar {
        let img = image::open(format!(
            "{}/{}",
            std::env::current_dir().unwrap().to_str().unwrap(),
            "images/bar.png"
        ))
        .unwrap()
        .to_rgba8();
        let pixels: Vec<u8> = img.into_vec();

        // TODO: make a way to change titlebar colors
        let bar = dakota.create_resource().unwrap();
        dakota
            .define_resource_from_bits(
                &bar,
                pixels.as_slice(),
                64,
                64,
                0,
                dak::dom::Format::ARGB8888,
            )
            .unwrap();

        let img = image::open(format!(
            "{}/{}",
            std::env::current_dir().unwrap().to_str().unwrap(),
            "images/dot.png"
        ))
        .unwrap()
        .to_bgra8();
        let pixels: Vec<u8> = img.into_vec();
        let dot = dakota.create_resource().unwrap();
        dakota
            .define_resource_from_bits(
                &dot,
                pixels.as_slice(),
                64,
                64,
                0,
                dak::dom::Format::ARGB8888,
            )
            .unwrap();

        Titlebar { bar: bar, dot: dot }
    }

    /// Called when the swapchain image resizes
    pub fn handle_ood(&mut self, dakota: &mut dak::Dakota) {
        dakota.set_size(
            &self.wm_desktop,
            dom::RelativeSize {
                width: dom::Value::Relative(dom::Relative::new(1.0)),
                height: dom::Value::Constant(dom::Constant::new(
                    dakota.get_resolution().1 as i32 - MENUBAR_SIZE,
                )),
            },
        );
    }

    /// Returns an ID for an element bound with a defaul texture resource
    fn get_default_cursor(dakota: &mut dak::Dakota) -> DakotaId {
        let image = dakota.create_resource().unwrap();
        dakota
            .define_resource_from_image(
                &image,
                std::path::Path::new("images/cursor.png"),
                dom::Format::ARGB8888,
            )
            .expect("Could not import background image into dakota");
        let surf = dakota.create_element().unwrap();
        dakota.set_offset(
            &surf,
            dom::RelativeOffset {
                x: dom::Value::Constant(dom::Constant::new(0)),
                y: dom::Value::Constant(dom::Constant::new(0)),
            },
        );
        dakota.set_size(
            &surf,
            dom::RelativeSize {
                width: dom::Value::Constant(dom::Constant::new(10)),
                height: dom::Value::Constant(dom::Constant::new(15)),
            },
        );
        dakota.set_resource(&surf, image.clone());

        surf
    }

    /// Define all of the Dakota elements that make up the menu bar
    /// at the top of the screen
    fn create_menubar(dakota: &mut dak::Dakota, menubar_font: DakotaId) -> DakotaId {
        let barcolor = dakota.create_resource().unwrap();
        dakota.set_resource_color(&barcolor, dak::dom::Color::new(0.085, 0.09, 0.088, 0.9));

        let menubar = dakota.create_element().unwrap();
        dakota.set_size(
            &menubar,
            dom::RelativeSize {
                // Make our bar 16 px tall but stretch across the screen
                width: dom::Value::Relative(dom::Relative::new(1.0)),
                height: dom::Value::Constant(dom::Constant::new(MENUBAR_SIZE)),
            },
        );
        dakota.set_resource(&menubar, barcolor);

        let name = dakota.create_element().unwrap();
        dakota.set_text_regular(&name, "Category5");
        dakota.set_text_font(&name, menubar_font);
        dakota.add_child_to_element(&menubar, name);

        return menubar;
    }

    /// Refresh the date and time string in the menubar
    ///
    /// This should be called every time change.
    pub fn refresh_datetime(&mut self, dakota: &mut dak::Dakota) {
        let date = chrono::Local::now();
        // https://docs.rs/chrono-wasi07/latest/chrono/format/strftime/index.html
        dakota.set_text_regular(
            &self.wm_datetime,
            &date.format("%a %B %e %l:%M %p").to_string(),
        );
        dakota.set_text_font(&self.wm_datetime, self.wm_menubar_font.clone());
        log::error!(
            "Updated time to: {}",
            date.format("%a %B %e %l:%M %p").to_string()
        );
    }

    /// Create a new WindowManager
    ///
    /// This will create all the graphical resources needed for
    /// the compositor. The WindowManager will create and own
    /// the Thundr, thereby readying the display to draw.
    pub fn new(dakota: &mut dak::Dakota, atmos: &mut Atmosphere) -> WindowManager {
        #[cfg(feature = "renderdoc")]
        let doc = RenderDoc::new().unwrap();

        // Tell the atmosphere rend's resolution
        let res = dakota.get_resolution();
        atmos.set_resolution(res.0, res.1);
        let (major, minor) = dakota.get_drm_dev();
        atmos.set_drm_dev(major, minor);

        // Create a DOM object that all others will hang off of
        // ------------------------------------------------------------------
        let root = dakota.create_element().unwrap();
        // Manually set the size to the parent container so that its size
        // isn't derived from the image we set as the desktop background
        dakota.set_size(
            &root,
            dom::RelativeSize {
                width: dom::Value::Relative(dom::Relative::new(1.0)),
                height: dom::Value::Relative(dom::Relative::new(1.0)),
            },
        );

        let dom = dakota.create_dakota_dom().unwrap();
        dakota.set_dakota_dom(
            &dom,
            dak::dom::DakotaDOM {
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
            },
        );

        // First create our menu bar across the top of the screen
        // ------------------------------------------------------------------
        let menubar_font = dakota.create_font_instance().unwrap();
        let menubar = Self::create_menubar(dakota, menubar_font.clone());
        dakota.add_child_to_element(&root, menubar.clone());

        dakota.define_font(
            &menubar_font,
            dom::Font {
                name: "Default Font".to_string(),
                path: "./JetBrainsMono-Regular.ttf".to_string(),
                pixel_size: 16,
                color: Some(dom::Color {
                    r: 0.941,
                    g: 0.921,
                    b: 0.807,
                    a: 1.0,
                }),
            },
        );
        let datetime = dakota.create_element().unwrap();
        dakota.set_content(
            &menubar,
            dom::Content {
                el: datetime.clone(),
            },
        );

        // Next add a dummy element to place all of the client window child elements
        // inside of.
        // ------------------------------------------------------------------
        let desktop = dakota.create_element().unwrap();
        dakota.add_child_to_element(&root, desktop.clone());
        // set the background for this desktop
        let image = dakota.create_resource().unwrap();
        dakota
            .define_resource_from_image(
                &image,
                std::path::Path::new("images/cat5_desktop.png"),
                dom::Format::ARGB8888,
            )
            .expect("Could not import background image into dakota");
        dakota.set_resource(&desktop, image);

        // now add a cursor on top of this
        // ------------------------------------------------------------------
        let cursor = WindowManager::get_default_cursor(dakota);
        dakota.add_child_to_element(&root, cursor.clone());

        let mut ret = WindowManager {
            wm_titlebar: WindowManager::get_default_titlebar(dakota),
            wm_cursor: Some(cursor.clone()),
            wm_cursor_hotspot: (0, 0),
            wm_default_cursor: cursor,
            wm_dakota_root: root,
            wm_dakota_dom: dom,
            wm_menubar: menubar,
            wm_menubar_font: menubar_font,
            wm_datetime: datetime,
            wm_desktop: desktop,
            wm_apps: PropertyList::new(),
            wm_will_die: Vec::new(),
            wm_atmos_ids: Vec::new(),
            #[cfg(feature = "renderdoc")]
            wm_renderdoc: doc,
        };
        ret.refresh_datetime(dakota);
        // This sets the desktop size
        ret.handle_ood(dakota);

        return ret;
    }

    /// Set the desktop background for the renderer
    ///
    /// This basically just creates a image with the max
    /// depth that takes up the entire screen.
    fn set_background_from_mem(
        dakota: &mut dak::Dakota,
        elem: &DakotaId,
        texture: &[u8],
        tex_width: u32,
        tex_height: u32,
    ) {
        let image = dakota.create_resource().unwrap();
        dakota
            .define_resource_from_bits(
                &image,
                texture,
                tex_width,
                tex_height,
                0,
                dak::dom::Format::ARGB8888,
            )
            .unwrap();
        dakota.set_resource(elem, image);
    }

    /// Add a image to the renderer to be displayed.
    ///
    /// The imagees are added to a list, and will be individually
    /// dispatched for drawing later.
    ///
    /// Images need to be in an indexed vertex format.
    ///
    /// tex_res is the resolution of `texture`
    /// window_res is the size of the on screen window
    fn create_window(&mut self, dakota: &mut dak::Dakota, id: WindowId) -> Result<()> {
        log::info!("wm: Creating new window {:?}", id);
        // This surface will have its dimensions updated during recording
        let surf = dakota.create_element().unwrap();
        // The bar should be a percentage of the screen height
        //let barsize = atmos.get_barsize();

        // TODO: Server side decorations
        // draw buttons on the titlebar
        // ----------------------------------------------------------------
        //let dims = Self::get_dot_dims(barsize, &(8.0, 8.0));
        //let mut dot = self
        //    .wm_thundr
        //    .create_surface(dims.0, dims.1, dims.2, dims.3);
        //self.wm_thundr
        //    .bind_image(&mut dot, self.wm_titlebar.dot.clone());
        //// add the dot as a subsurface above the window
        //surf.add_subsurface(dot);
        //// ----------------------------------------------------------------

        //// now render the bar itself, as wide as the window
        //// the bar needs to be behind the dots
        //// ----------------------------------------------------------------
        //let dims = Self::get_bar_dims(barsize, &(8.0, 8.0));
        //let mut bar = self
        //    .wm_thundr
        //    .create_surface(dims.0, dims.1, dims.2, dims.3);
        //self.wm_thundr
        //    .bind_image(&mut bar, self.wm_titlebar.bar.clone());
        //surf.add_subsurface(bar);
        // ----------------------------------------------------------------

        self.wm_apps.update_or_create(
            id.into(),
            App {
                a_id: id,
                a_marked_for_death: false,
                a_surf: surf,
                a_image: None,
                a_ssd: None,
            },
        );
        Ok(())
    }

    /// Handles an update from dmabuf task
    ///
    /// Translates the task update structure into lower
    /// level calls to import a dmabuf and update a image.
    /// Creates a new image if one doesn't exist yet.
    fn update_window_contents_from_dmabuf(
        &mut self,
        atmos: &mut Atmosphere,
        dakota: &mut dak::Dakota,
        info: &UpdateWindowContentsFromDmabuf,
    ) -> Result<()> {
        log::debug!("Updating window {:?} with {:#?}", info.ufd_id, info);
        // Find the app corresponding to that window id
        // don't use a helper here because of the borrow checker
        let mut app = match self.wm_apps[info.ufd_id.into()].as_mut() {
            Some(a) => a,
            // If the id is not found, then don't update anything
            None => return Err(anyhow!("Could not find id {:?}", info.ufd_id)),
        };

        // Create a new resource from this dmabuf
        let res = dakota.create_resource().unwrap();
        dakota
            .define_resource_from_dmabuf(
                &res,
                info.ufd_dmabuf.db_fd.try_clone().unwrap(),
                info.ufd_dmabuf.db_plane_idx,
                info.ufd_dmabuf.db_offset,
                info.ufd_dmabuf.db_width,
                info.ufd_dmabuf.db_height,
                info.ufd_dmabuf.db_stride,
                info.ufd_dmabuf.db_mods,
                Some(Box::new(DmabufReleaseInfo {
                    dr_fd: info.ufd_dmabuf.db_fd.try_clone()?,
                    dr_wl_buffer: info.ufd_wl_buffer.clone(),
                })),
            )
            .unwrap();
        dakota.set_resource(&app.a_surf, res.clone());
        app.a_image = Some(res);

        if let Some(damage) = atmos.take_buffer_damage(info.ufd_id) {
            if let Some(image) = app.a_image.as_ref() {
                dakota.damage_resource(image, damage);
            }
        }
        if let Some(damage) = atmos.take_surface_damage(info.ufd_id) {
            dakota.damage_element(&app.a_surf, damage);
        }

        Ok(())
    }

    /// Handle update from memimage task
    ///
    /// Copies the shm buffer into the app's image.
    /// Creates a new image if one doesn't exist yet.
    fn update_window_contents_from_mem(
        &mut self,
        atmos: &mut Atmosphere,
        dakota: &mut dak::Dakota,
        info: &UpdateWindowContentsFromMem,
    ) -> Result<()> {
        log::debug!("Updating window {:?} with {:#?}", info.id, info);
        // Find the app corresponding to that window id
        // don't use a helper here because of the borrow checker
        let mut app = match self.wm_apps[info.id.into()].as_mut() {
            Some(a) => a,
            // If the id is not found, then don't update anything
            None => return Err(anyhow!("Could not find id {:?}", info.id)),
        };

        let buffer_damage = atmos.take_buffer_damage(info.id);
        let surface_damage = atmos.take_surface_damage(info.id);
        // Damage the image
        if let Some(damage) = buffer_damage {
            if let Some(image) = app.a_image.as_ref() {
                dakota.damage_resource(image, damage);
            }
        }
        if let Some(damage) = surface_damage {
            dakota.damage_element(&app.a_surf, damage);
        }

        // Create a new image, the old one will have its refcount dropped
        // TODO: add update method here
        let res = dakota.create_resource().unwrap();
        dakota
            .define_resource_from_bits(
                &res,
                &info.pixels,
                info.width as u32,
                info.height as u32,
                0,
                dak::dom::Format::ARGB8888,
            )
            .unwrap();
        dakota.set_resource(&app.a_surf, res.clone());
        app.a_image = Some(res);

        Ok(())
    }

    /// We have to pass in the barsize to get around some annoying borrow checker stuff
    fn get_bar_dims(barsize: f32, surface_size: &(f32, f32)) -> (f32, f32, f32, f32) {
        (
            // align it at the top right
            0.0,
            // draw the bar above the window
            -barsize,
            // the bar is as wide as the window
            surface_size.0,
            // use a percentage of the screen size
            barsize,
        )
    }

    fn get_dot_dims(barsize: f32, surface_size: &(f32, f32)) -> (f32, f32, f32, f32) {
        // The dotsize should be just slightly smaller
        let dotsize = barsize * 0.95;

        (
            surface_size.0
                // we don't want to go past the end of the bar
                    - barsize,
            -barsize,
            // align it at the top right
            dotsize, // width
            dotsize, // height
        )
    }

    /// Flag this window to be killed.
    ///
    /// This adds it to our death list, which will be reaped next frame after
    /// we are done using its resources.
    fn close_window(
        &mut self,
        atmos: &mut Atmosphere,
        dakota: &mut dak::Dakota,
        id: WindowId,
    ) -> Result<()> {
        // If this window doesn't exist then we might be closing
        // it before we have ever set it up.
        if !self.wm_apps.id_exists(id.into()) {
            log::debug!(
                "WARN: closing window {:?} but we don't have an app entry for it",
                id
            );
            return Ok(());
        }

        log::debug!("Closing window {:?}", id);

        {
            let mut app = self.lookup_app_from_id_mut(id)?;
            app.a_marked_for_death = true;
        }

        let app = self.lookup_app_from_id(id)?;
        // remove this surface in case it is a toplevel window
        dakota.remove_child_from_element(&self.wm_desktop, &app.a_surf)?;
        // If this is a subsurface, remove it from its parent
        if let Some(parent) = atmos.get_parent_window(id) {
            let parent_app = self.lookup_app_from_id(parent)?;
            dakota.remove_child_from_element(&parent_app.a_surf, &app.a_surf)?;
        }
        self.wm_will_die.push(id);

        Ok(())
    }

    /// Remove any apps marked for death. Usually we can't remove
    /// a window immediately because its image(s) are still being
    /// used by thundr
    fn reap_dead_windows(&mut self) {
        // Only retain alive windows in the array
        for id in self.wm_will_die.drain(..) {
            if let Some(app) = self.wm_apps[id.into()].as_ref() {
                assert!(app.a_marked_for_death);

                self.wm_apps.deactivate(id.into())
            }
        }
    }

    /// A helper that uses a window id to do a static lookup of
    /// vkcomp's App structure. The index for the App structure matches
    /// the window id number
    fn lookup_app_from_id<'a>(&'a self, id: WindowId) -> Result<&'a App> {
        match self.wm_apps[id.into()].as_ref() {
            Some(a) => Ok(a),
            // app must have been closed
            None => Err(anyhow!("Could not find id {:?} in app list", id)),
        }
    }

    fn lookup_app_from_id_mut<'a>(&'a mut self, id: WindowId) -> Result<&'a mut App> {
        match self.wm_apps[id.into()].as_mut() {
            Some(a) => Ok(a),
            // app must have been closed
            None => Err(anyhow!("Could not find id {:?} in app list", id)),
        }
    }

    /// Move the window to the front of the scene
    ///
    /// There is really only one toplevel window movement
    /// event: moving something to the top of the window stack
    /// when the user clicks on it and puts it into focus.
    fn move_to_front(
        &mut self,
        atmos: &mut Atmosphere,
        dakota: &mut dak::Dakota,
        win: WindowId,
    ) -> Result<()> {
        // get and use the root window for this subsurface
        // in case it is a subsurface.
        let root = match atmos.get_root_window(win) {
            Some(parent) => parent,
            None => win,
        };
        let surf = &self.lookup_app_from_id(root)?.a_surf;

        // Move this surface to the front child of the window parent
        dakota
            .move_child_to_front(&self.wm_desktop, surf)
            .context(format!("Moving window {:?} to the front", win))?;

        Ok(())
    }

    /// Add a new toplevel surface
    ///
    /// This maps a new toplevel surface and places it in the desktop. This
    /// is where the dakota element is added to the desktop as a child.
    fn new_toplevel(&mut self, dakota: &mut dak::Dakota, win: WindowId) -> Result<()> {
        let surf = self
            .lookup_app_from_id(win)
            .context("Could not find window")?
            .a_surf
            .clone();
        // We might have not added this element to the desktop, moving to front
        // as part of focus is one of the first things that happens when a
        // new window is created
        dakota.add_child_to_element(&self.wm_desktop, surf);

        Ok(())
    }

    /// Update the current cursor image
    ///
    /// Wayland clients may assign a surface to serve as the cursor image.
    /// Here we update the current cursor.
    fn set_cursor(
        &mut self,
        atmos: &mut Atmosphere,
        dakota: &mut dak::Dakota,
        win: Option<WindowId>,
        hotspot: (i32, i32),
    ) -> Result<()> {
        if let Some(old) = self.wm_cursor.as_ref() {
            dakota.remove_child_from_element(&self.wm_dakota_root, old)?;
            self.wm_cursor_hotspot = (0, 0);
        }

        // Clear the cursor if the client unset it. Otherwise get the
        // new surface, add it as a child and set it.
        self.wm_cursor = match win {
            Some(win) => {
                let surf = self.lookup_app_from_id(win)?.a_surf.clone();
                dakota.add_child_to_element(&self.wm_dakota_root, surf.clone());
                // Set the size of the cursor
                let surface_size = atmos.get_surface_size(win);
                dakota.set_size(
                    &surf,
                    dom::RelativeSize {
                        width: dom::Value::Constant(dom::Constant::new(surface_size.0 as i32)),
                        height: dom::Value::Constant(dom::Constant::new(surface_size.1 as i32)),
                    },
                );
                self.wm_cursor_hotspot = hotspot;
                Some(surf)
            }
            None => None,
        };

        Ok(())
    }

    /// Reset the cursor to the default.
    ///
    /// Used when we are no longer listening to the client's suggested
    /// cursor
    fn reset_cursor(&mut self, dakota: &mut dak::Dakota) -> Result<()> {
        if let Some(old) = self.wm_cursor.as_ref() {
            dakota.remove_child_from_element(&self.wm_dakota_root, old)?;
        }

        dakota.add_child_to_element(&self.wm_dakota_root, self.wm_default_cursor.clone());
        self.wm_cursor = Some(self.wm_default_cursor.clone());
        self.wm_cursor_hotspot = (0, 0);

        Ok(())
    }

    /// Adds a new subsurface to the parent.
    ///
    /// The new subsurface will be moved to the top of the subsurface
    /// stack, as this is the default. The position may later be changed
    /// through the wl_subsurface interface.
    fn new_subsurface(
        &mut self,
        dakota: &mut dak::Dakota,
        win: WindowId,
        parent: WindowId,
    ) -> Result<()> {
        let surf = &self.lookup_app_from_id(win)?.a_surf;
        let parent_surf = &self.lookup_app_from_id(parent)?.a_surf;

        dakota.add_child_to_element(parent_surf, surf.clone());
        // Under normal operation Dakota elements are restricted to the size of
        // their parent. We do not want this for XDG popup surfaces, which are
        // layered ontop of toplevel windows but are not restricted by their
        // bounds. This special attribute tells dakota not to clip them.
        dakota.set_unbounded_subsurface(surf, true);
        Ok(())
    }

    /// Look up and place a surface above another in the subsurface list
    ///
    /// win will be placed above other. The parent will be looked up by
    /// searching the root window from atmos.
    fn subsurf_place_above(
        &mut self,
        atmos: &mut Atmosphere,
        dakota: &mut dak::Dakota,
        win: WindowId,
        other: WindowId,
    ) -> Result<()> {
        self.subsurf_reorder_common(atmos, dakota, dak::SubsurfaceOrder::Above, win, other)
    }

    /// Same as above, but place the subsurface below other.
    fn subsurf_place_below(
        &mut self,
        atmos: &mut Atmosphere,
        dakota: &mut dak::Dakota,
        win: WindowId,
        other: WindowId,
    ) -> Result<()> {
        self.subsurf_reorder_common(atmos, dakota, dak::SubsurfaceOrder::Below, win, other)
    }

    fn subsurf_reorder_common(
        &mut self,
        atmos: &mut Atmosphere,
        dakota: &mut dak::Dakota,
        order: dak::SubsurfaceOrder,
        win: WindowId,
        other: WindowId,
    ) -> Result<()> {
        let surf = &self.lookup_app_from_id(win)?.a_surf;
        let other_surf = &self.lookup_app_from_id(other)?.a_surf;
        let root = atmos
            .get_root_window(win)
            .expect("The window should have a root since it is a subsurface");
        let root_surf = &self.lookup_app_from_id(root)?.a_surf;

        dakota.reorder_children_element(root_surf, order, surf, other_surf)?;

        Ok(())
    }

    /// Dispatch window management tasks
    ///
    /// This is where we handle things like surface/element creation, window creation and
    /// destruction, etc.
    pub fn process_task(&mut self, atmos: &mut Atmosphere, dakota: &mut dak::Dakota, task: &Task) {
        log::info!("wm: got task {:?}", task);
        let err = match task {
            // create new window
            Task::create_window(id) => self
                .create_window(dakota, *id)
                .context("Task: create_window"),
            Task::move_to_front(id) => self
                .move_to_front(atmos, dakota, *id)
                .context("Task: Moving window to front"),
            Task::new_subsurface { id, parent } => self
                .new_subsurface(dakota, *id, *parent)
                .context("Task: new_subsurface"),
            Task::place_subsurface_above { id, other } => self
                .subsurf_place_above(atmos, dakota, *id, *other)
                .context("Task: place_subsurface_above"),
            Task::place_subsurface_below { id, other } => self
                .subsurf_place_below(atmos, dakota, *id, *other)
                .context("Task: place_subsurface_below"),
            Task::close_window(id) => self
                .close_window(atmos, dakota, *id)
                .context("Task: close_window"),
            Task::new_toplevel(id) => self.new_toplevel(dakota, *id).context("Task: new_toplevel"),
            Task::set_cursor { id, hotspot } => self
                .set_cursor(atmos, dakota, *id, *hotspot)
                .context("Task: set_cursor"),
            Task::reset_cursor => self.reset_cursor(dakota).context("Task: reset_cursor"),
            // update window from gpu buffer
            Task::uwcfd(uw) => self
                .update_window_contents_from_dmabuf(atmos, dakota, uw)
                .context(format!("Task: Updating window {:?} from dmabuf", uw.ufd_id)),
            // update window from shm
            Task::uwcfm(uw) => self
                .update_window_contents_from_mem(atmos, dakota, uw)
                .context(format!(
                    "Task: Updating window {:?} from shared memory",
                    uw.id
                )),
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
    fn record_draw(&mut self, atmos: &mut Atmosphere, dakota: &mut dak::Dakota) {
        // get the latest cursor position
        // ----------------------------------------------------------------
        let (cursor_x, cursor_y) = atmos.get_cursor_pos();
        log::profiling!("Drawing cursor at ({}, {})", cursor_x, cursor_y);
        if let Some(cursor) = self.wm_cursor.as_mut() {
            dakota.set_offset(
                &cursor,
                dom::RelativeOffset {
                    x: dom::Value::Constant(dom::Constant::new(
                        (cursor_x as i32).saturating_sub(self.wm_cursor_hotspot.0),
                    )),
                    y: dom::Value::Constant(dom::Constant::new(
                        (cursor_y as i32).saturating_sub(self.wm_cursor_hotspot.1),
                    )),
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
        atmos.map_inorder_on_surfs(|id| {
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
        for r_id in self.wm_atmos_ids.iter() {
            let id = *r_id;
            // Now render the windows
            let a = self.wm_apps[id.into()]
                .as_mut()
                .expect(format!("Could not find id {:?} to record for drawing", id).as_str());

            // If this window has been closed or if it is not ready for
            // rendering, ignore it
            if a.a_marked_for_death || !atmos.get_window_in_use(a.a_id) {
                return;
            }

            // get parameters
            // ----------------------------------------------------------------
            let surface_pos = atmos.get_surface_pos(a.a_id);
            let surface_size = atmos.get_surface_size(a.a_id);
            log::debug!(
                "placing dakota element at {:?} with size {:?}",
                surface_pos,
                surface_size
            );

            // update the th::Surface pos and size
            dakota.set_offset(
                &a.a_surf,
                dom::RelativeOffset {
                    x: dom::Value::Constant(dom::Constant::new(surface_pos.0 as i32)),
                    y: dom::Value::Constant(dom::Constant::new(surface_pos.1 as i32)),
                },
            );
            dakota.set_size(
                &a.a_surf,
                dom::RelativeSize {
                    width: dom::Value::Constant(dom::Constant::new(surface_size.0 as i32)),
                    height: dom::Value::Constant(dom::Constant::new(surface_size.1 as i32)),
                },
            );
            // ----------------------------------------------------------------

            // Send any pending frame callbacks
            atmos.send_frame_callbacks_for_surf(a.a_id);

            // Only display the bar for toplevel surfaces
            // i.e. don't for popups
            //if atmos.get_toplevel(id) {
            //    // The bar should be a percentage of the screen height
            //    let barsize = atmos.get_barsize();

            //    // Each toplevel window has two subsurfaces (in thundr): the
            //    // window bar and the window dot. If it's toplevel we are drawing SSD,
            //    // so we need to update the positions of these as well.
            //    let mut sub = a.a_surf.get_subsurface(0);
            //    let dims = Self::get_dot_dims(barsize, &surface_size);
            //    sub.set_pos(dims.0, dims.1);
            //    sub.set_size(dims.2, dims.3);

            //    let dims = Self::get_bar_dims(barsize, &surface_size);
            //    let mut sub = a.a_surf.get_subsurface(0);
            //    sub.set_pos(dims.0, dims.1);
            //    sub.set_size(dims.2, dims.3);
            //}
        }
    }

    /// The main event loop of the vkcomp thread
    pub fn render_frame(&mut self, dakota: &mut dak::Dakota, atmos: &mut Atmosphere) -> Result<()> {
        // how much time is spent drawing/presenting
        let mut draw_stop = StopWatch::new();

        #[cfg(feature = "renderdoc")]
        if atmos.get_renderdoc_recording() {
            self.wm_renderdoc
                .start_frame_capture(std::ptr::null(), std::ptr::null());
        }

        // iterate through all the tasks that ways left
        // us in this hemisphere
        //  (aka process the work queue)
        while let Some(task) = atmos.get_next_wm_task() {
            self.process_task(atmos, dakota, &task);
        }

        // start recording how much time we spent doing graphics
        log::debug!("_____________________________ FRAME BEGIN");
        // Create a frame out of the hemisphere we got from ways
        draw_stop.start();

        // Update our surface locations in Dakota
        //
        // Check if there are updates from wayland before doing this, since updating
        // the dakota elements triggers a full redraw
        if atmos.is_changed() {
            self.record_draw(atmos, dakota);
            atmos.clear_changed();
        }

        // Rerun rendering until it succeeds, this will handle out of date swapchains
        loop {
            match dakota.dispatch_rendering(&self.wm_dakota_dom) {
                Ok(()) => break,
                // Do not call handle_ood here. This will propogate the error back up to
                // the main event loop which will call back to us from there.
                Err(e) => return Err(e),
            };
        }

        draw_stop.end();
        log::debug!(
            "spent {} ms drawing this frame",
            draw_stop.get_duration().as_millis()
        );

        atmos.print_surface_tree();

        #[cfg(feature = "renderdoc")]
        if atmos.get_renderdoc_recording() {
            self.wm_renderdoc
                .end_frame_capture(std::ptr::null(), std::ptr::null());
        }
        self.reap_dead_windows();
        atmos.release_consumables();

        log::debug!("_____________________________ FRAME END");

        Ok(())
    }
}
