/// Dakota UI Toolkit
///
/// Dakota is a UI toolkit designed for rendering trees of surfaces. These
/// surfaces can be easily expressed in XML documents, and updated dynamically
/// by the application.
///
/// Austin Shafer - 2022
extern crate freetype as ft;
extern crate image;
extern crate lluvia as ll;
extern crate thundr as th;
pub use th::ThundrError as DakotaError;
pub use th::{Damage, Dmabuf, DmabufPlane, Droppable, MappedImage};

extern crate bitflags;

extern crate lazy_static;
extern crate utils;
use utils::log;
pub use utils::MemImage;
pub use utils::{
    anyhow, fdwatch::FdWatch, region::Rect, timing::StopWatch, Context, Error, Result,
};

pub mod dom;
pub mod input;
#[cfg(test)]
mod tests;
pub use crate::input::{Keycode, MouseButton};
mod platform;
use platform::Platform;
pub mod xml;

pub mod event;
use event::EventSystem;
pub use event::{AxisSource, Event, RawKeycode};
mod layout;
use layout::{LayoutNode, LayoutSpace};
mod render;

mod font;
use font::*;

// Re-exmport our getters/setters
mod generated;

use std::ops::Deref;
use std::os::fd::RawFd;

pub type DakotaId = ll::Entity;

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum DakotaObjectType {
    Element,
    DakotaDOM,
    Resource,
    Font,
}

/// Only one of content or children may be defined,
/// they are mutually exclusive.
///
/// Element layout will:
///   a) expand horizontally to fit their container
///   b) expand vertically to fit their container
///   c) a element's content is scaled to fit the element.
///   d) default behavior is only vertical scrolling allowed for
///      when the element's content is longer than the element's height.
///      d.1) if the user does not specify a vertical/horizontal scrolling,
///           then that edge of the element is static. It is basically
///           a window, and scrolling may occur within that element in
///           whatever dimensions were not marked as scrolling.
///           (e.g. default behavior is a horizontal scrolling = false
///            and vertical scrolling = true)
///   e) a-b may be limited by dimensions specified by the user.
///      the dimensions are not specified, then the resource's
///      default size is used.
///   f) regarding (e), if the element's size does not fill the container,
///      then:
///      f.1) the elementes will be laid out horizontally first,
///      f.2) with vertical wrapping if there is not enough room.
pub struct Dakota {
    // GROSS: we need thund to be before plat so that it gets dropped first
    // It might reference the window inside plat, and will segfault if
    // dropped after it.
    d_thund: th::Thundr,
    /// The current window system backend.
    ///
    /// This may be SDL2 for windowed systems, or direct2display. This handles platform-specific
    /// initialization.
    d_plat: Box<dyn Platform>,
    /// This is one ECS that is composed of multiple tables
    d_ecs_inst: ll::Instance,
    /// This is all of the LayoutNodes in the system, each corresponding to
    /// an Element or a subcomponent of an Element. Indexed by DakotaId.
    d_layout_nodes: ll::Component<LayoutNode>,
    // NOTE: --------------------------------
    //
    // If you update the following you may have to edit the generated
    // getters/setters in generated.rs
    d_node_types: ll::Component<DakotaObjectType>,

    // Resource components
    // --------------------------------------------
    /// The resource info configured by the user
    d_resource_hints: ll::Component<dom::Hints>,
    d_resource_thundr_image: ll::Component<th::Image>,
    d_resource_color: ll::Component<dom::Color>,

    // Element components
    // --------------------------------------------
    /// The resource currently assigned to this element
    d_resources: ll::Component<DakotaId>,
    d_offsets: ll::Component<dom::RelativeOffset>,
    d_widths: ll::Component<dom::Value>,
    d_heights: ll::Component<dom::Value>,
    d_fonts: ll::Component<dom::Font>,
    d_texts: ll::Component<dom::Text>,
    /// points to an id with font instance
    d_text_font: ll::Component<DakotaId>,
    d_contents: ll::Component<dom::Content>,
    d_bounds: ll::Component<dom::Edges>,
    d_children: ll::Component<Vec<DakotaId>>,
    d_unbounded_subsurf: ll::Component<bool>,
    /// Any viewports assigned
    ///
    /// If this is a viewport boundary then this will be populated to
    /// control draw clipping
    d_viewports: ll::Component<th::Viewport>,

    // DOM components
    // --------------------------------------------
    d_dom: ll::Component<dom::DakotaDOM>,

    /// This is the root node in the scene tree
    d_layout_tree_root: Option<DakotaId>,
    d_window_dims: Option<(u32, u32)>,
    d_needs_redraw: bool,
    d_needs_refresh: bool,
    d_event_sys: EventSystem,
    /// Default Font instance
    d_default_font_inst: DakotaId,
    d_freetype: ft::Library,
    d_ood_counter: usize,

    /// Font shaping information. This is held separately outside of our ECS tables
    /// since it is not threadsafe. This associates a Font with the corresponding
    /// instance containing the shaping information.
    d_font_instances: Vec<(dom::Font, FontInstance)>,

    /// Cached mouse position
    ///
    /// Mouse updates are relative, so we need to add them to the last
    /// known mouse location. That is the value stored here.
    d_mouse_pos: (i32, i32),
}

/// Enum for specifying subsurface operations
pub enum SubsurfaceOrder {
    Above,
    Below,
}

macro_rules! create_component_and_table {
    ($ecs:ident, $llty:ty, $name:ident) => {
        let $name: ll::Component<$llty> = $ecs.add_component();
    };
}

impl Dakota {
    /// Helper for initializing Thundr for a given platform.
    fn init_thundr(plat: &mut Box<dyn Platform>) -> Result<(th::Thundr, (i32, i32))> {
        let info = th::CreateInfo::builder()
            .surface_type(plat.get_th_surf_type()?)
            .build();

        let thundr = th::Thundr::new(&info).context("Failed to initialize Thundr")?;

        let dpi = thundr
            .get_dpi()
            .context("Failed to get DPI during platform init")?;

        Ok((thundr, dpi))
    }

    /// Try initializing the different plaform backends until we find one that works
    ///
    /// This will test for platform support and initialize the platform, Thundr, and
    /// get the DPI of the display. These three are tested since they all may fail
    /// given different configurations. DPI fails if SDL2 tries to initialize us on
    /// a physical display.
    fn initialize_platform() -> Result<(Box<dyn Platform>, th::Thundr, (i32, i32))> {
        if std::env::var("DAKOTA_HEADLESS_BACKEND").is_err() {
            // If we are not forcing headless mode, start by attempting sdl
            #[cfg(feature = "sdl")]
            if std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok() {
                match platform::SDL2Plat::new() {
                    Ok(sdl) => {
                        let mut sdl: Box<dyn Platform> = Box::new(sdl);
                        match Self::init_thundr(&mut sdl) {
                            Ok((thundr, dpi)) => return Ok((sdl, thundr, dpi)),
                            Err(e) => log::error!("Failed to create SDL2 backend: {:?}", e),
                        }
                    }
                    Err(e) => log::error!("Failed to create new SDL platform instance: {:?}", e),
                };
            }

            #[cfg(feature = "direct2display")]
            if let Ok(display) = platform::DisplayPlat::new() {
                let mut display: Box<dyn Platform> = Box::new(display);
                match Self::init_thundr(&mut display) {
                    Ok((thundr, dpi)) => return Ok((display, thundr, dpi)),
                    Err(e) => log::error!("Failed to create Direct2Display backend: {:?}", e),
                }
            }
        }

        let headless = platform::HeadlessPlat::new();
        let mut display: Box<dyn Platform> = Box::new(headless);
        match Self::init_thundr(&mut display) {
            Ok((thundr, dpi)) => return Ok((display, thundr, dpi)),
            Err(e) => log::error!("Failed to create Headless backend: {:?}", e),
        }

        return Err(anyhow!("Could not find available platform"));
    }

    /// Construct a new Dakota instance
    ///
    /// This will initialize the window system platform layer, create a thundr
    /// instance from it, and wrap it in Dakota.
    pub fn new() -> Result<Self> {
        let (plat, thundr, _dpi) = Self::initialize_platform()?;

        let mut layout_ecs = ll::Instance::new();
        create_component_and_table!(layout_ecs, LayoutNode, layout_table);
        create_component_and_table!(layout_ecs, DakotaObjectType, types_table);
        create_component_and_table!(layout_ecs, dom::Hints, resource_hints_table);
        create_component_and_table!(layout_ecs, th::Image, resource_thundr_image_table);
        create_component_and_table!(layout_ecs, dom::Color, resource_color_table);
        create_component_and_table!(layout_ecs, DakotaId, resources_table);
        create_component_and_table!(layout_ecs, dom::RelativeOffset, offsets_table);
        create_component_and_table!(layout_ecs, dom::Value, width_table);
        create_component_and_table!(layout_ecs, dom::Value, height_table);
        create_component_and_table!(layout_ecs, dom::Text, texts_table);
        create_component_and_table!(layout_ecs, dom::Font, font_table);
        create_component_and_table!(layout_ecs, DakotaId, text_font_table);
        create_component_and_table!(layout_ecs, dom::Content, content_table);
        create_component_and_table!(layout_ecs, dom::Edges, bounds_table);
        create_component_and_table!(layout_ecs, Vec<DakotaId>, children_table);
        create_component_and_table!(layout_ecs, dom::DakotaDOM, dom_table);
        create_component_and_table!(layout_ecs, bool, unbounded_subsurf_table);
        create_component_and_table!(layout_ecs, th::Viewport, viewports_table);

        // Create a default Font instance
        let default_inst = layout_ecs.add_entity();

        let mut ret = Self {
            d_plat: plat,
            d_thund: thundr,
            d_ecs_inst: layout_ecs,
            d_layout_nodes: layout_table,
            d_node_types: types_table,
            d_resource_hints: resource_hints_table,
            d_resource_thundr_image: resource_thundr_image_table,
            d_resource_color: resource_color_table,
            d_resources: resources_table,
            d_offsets: offsets_table,
            d_widths: width_table,
            d_heights: height_table,
            d_fonts: font_table,
            d_texts: texts_table,
            d_text_font: text_font_table,
            d_contents: content_table,
            d_bounds: bounds_table,
            d_children: children_table,
            d_dom: dom_table,
            d_unbounded_subsurf: unbounded_subsurf_table,
            d_viewports: viewports_table,
            d_layout_tree_root: None,
            d_window_dims: None,
            d_needs_redraw: false,
            d_needs_refresh: false,
            d_event_sys: EventSystem::new(),
            d_default_font_inst: default_inst.clone(),
            d_freetype: ft::Library::init().context(anyhow!("Could not get freetype library"))?,
            d_ood_counter: 30,
            d_font_instances: Vec::new(),
            d_mouse_pos: (0, 0),
        };

        ret.d_node_types.set(&default_inst, DakotaObjectType::Font);
        ret.define_font(
            &default_inst,
            dom::Font {
                name: "Default Font".to_string(),
                path: "./JetBrainsMono-Regular.ttf".to_string(),
                pixel_size: 16,
                color: None,
            },
        );

        return Ok(ret);
    }

    /// Get the Lluvia ECS backing DakotaIds
    ///
    /// This allows for applications using this to create their
    /// own Components which are indexed by DakotaId.
    pub fn get_ecs_instance(&self) -> ll::Instance {
        self.d_ecs_inst.clone()
    }

    /// Do we need to refresh the layout tree and rerender
    fn needs_refresh(&self) -> bool {
        self.d_needs_refresh
            || self.d_node_types.is_modified()
            || self.d_resource_hints.is_modified()
            || self.d_resource_thundr_image.is_modified()
            || self.d_resource_color.is_modified()
            || self.d_resources.is_modified()
            || self.d_offsets.is_modified()
            || self.d_widths.is_modified()
            || self.d_heights.is_modified()
            || self.d_fonts.is_modified()
            || self.d_texts.is_modified()
            || self.d_text_font.is_modified()
            || self.d_contents.is_modified()
            || self.d_bounds.is_modified()
            || self.d_children.is_modified()
            || self.d_dom.is_modified()
            || self.d_unbounded_subsurf.is_modified()
    }

    fn clear_needs_refresh(&mut self) {
        self.d_needs_refresh = false;
        self.d_node_types.clear_modified();
        self.d_resource_hints.clear_modified();
        self.d_resource_thundr_image.clear_modified();
        self.d_resource_color.clear_modified();
        self.d_resources.clear_modified();
        self.d_offsets.clear_modified();
        self.d_widths.clear_modified();
        self.d_heights.clear_modified();
        self.d_fonts.clear_modified();
        self.d_texts.clear_modified();
        self.d_text_font.clear_modified();
        self.d_contents.clear_modified();
        self.d_bounds.clear_modified();
        self.d_children.clear_modified();
        self.d_dom.clear_modified();
        self.d_unbounded_subsurf.clear_modified();
    }

    /// Create a new toplevel Dakota DOM
    pub fn create_dakota_dom(&mut self) -> Result<DakotaId> {
        self.create_new_id_common(DakotaObjectType::DakotaDOM)
    }

    /// Create a new Dakota element
    pub fn create_element(&mut self) -> Result<DakotaId> {
        self.create_new_id_common(DakotaObjectType::Element)
    }

    /// Create a new Dakota resource
    pub fn create_resource(&mut self) -> Result<DakotaId> {
        self.create_new_id_common(DakotaObjectType::Resource)
    }

    /// Create a new Dakota Font
    pub fn create_font(&mut self) -> Result<DakotaId> {
        self.create_new_id_common(DakotaObjectType::Font)
    }

    /// Define a Font for text rendering
    ///
    /// This accepts a definition of a Font, including the name and location
    /// of the font file. This is then loaded into Dakota and text rendering
    /// is allowed with the font.
    pub fn define_font(&mut self, id: &DakotaId, font: dom::Font) {
        if self
            .d_font_instances
            .iter()
            .find(|(f, _)| *f == font)
            .is_none()
        {
            self.d_font_instances.push((
                font.clone(),
                FontInstance::new(&self.d_freetype, &font.path, font.pixel_size, font.color),
            ));
        }

        self.d_fonts.set(id, font);
    }

    /// Returns true if this element will have it's position chosen for it by
    /// Dakota's layout engine.
    pub fn child_uses_autolayout(&self, id: &DakotaId) -> bool {
        self.d_offsets.get(id).is_some()
    }

    /// Create a new Dakota Id
    ///
    /// The type of the new id must be specified. In Dakota, all objects are
    /// represented by an Id, the type of which is specified during creation.
    /// This type will assign the "role" of this id, and what data can be
    /// attached to it.
    fn create_new_id_common(&mut self, element_type: DakotaObjectType) -> Result<DakotaId> {
        let id = self.d_ecs_inst.add_entity();

        self.d_node_types.set(&id, element_type);
        return Ok(id);
    }

    /// Define a resource's contents given a PNG image
    ///
    /// This will look up and open the image at `file_path`, and populate
    /// the resource `res`'s contents from it.
    pub fn define_resource_from_image(
        &mut self,
        res: &DakotaId,
        file_path: &std::path::Path,
        format: dom::Format,
    ) -> Result<()> {
        if self.is_resource_defined(res) {
            return Err(anyhow!("Cannot redefine Resource contents"));
        }

        // Create an in-memory representation of the image contents
        let resolution = image::image_dimensions(file_path)
            .context("Format of image could not be guessed correctly. Could not get resolution")?;
        let img = image::open(file_path)
            .context("Could not open image path")?
            .to_bgra8();
        let pixels: Vec<u8> = img.into_vec();

        self.define_resource_from_bits(
            res,
            pixels.as_slice(),
            resolution.0,
            resolution.1,
            0,
            format,
        )
    }

    /// Has this Resource been defined
    ///
    /// If a resource has been defined then it contains surface contents. This
    /// means an internal GPU resource has been allocated for it.
    pub fn is_resource_defined(&self, res: &DakotaId) -> bool {
        self.d_resource_thundr_image.get(res).is_some() || self.d_resource_color.get(res).is_some()
    }

    /// Define a resource's contents from an array
    ///
    /// This will initialize the resource's GPU image using the contents from
    /// the `data` slice. The `stride` and `format` arguments are used to correctly
    /// specify the layout of memory within `data`, a stride of zero implies that
    /// pixels are tightly packed.
    ///
    /// A stride of zero implies the pixels are tightly packed.
    pub fn define_resource_from_bits(
        &mut self,
        res: &DakotaId,
        data: &[u8],
        width: u32,
        height: u32,
        stride: u32, // TODO: Handle stride properly
        format: dom::Format,
    ) -> Result<()> {
        if format != dom::Format::ARGB8888 {
            return Err(anyhow!("Invalid image format"));
        }

        if self.is_resource_defined(res) {
            return Err(anyhow!("Cannot redefine Resource contents"));
        }

        // create a thundr image for each resource
        let image = self
            .d_thund
            .create_image_from_bits(data, width, height, stride, None)
            .context("Could not create Image resources")?;

        self.d_resource_thundr_image.set(res, image);
        Ok(())
    }

    /// Update the resource contents from a damaged CPU buffer
    ///
    /// This allows for updating the contents of a resource according to
    /// the data provided, within the damage regions specified. This is
    /// useful for compositor users to update a local copy of a shm texture.
    /// This should *NOT* be used with dmabuf-backed textures.
    pub fn update_resource_from_bits(
        &mut self,
        res: &DakotaId,
        data: &[u8],
        width: u32,
        height: u32,
        stride: u32, // TODO: Handle stride properly
        format: dom::Format,
        damage: Option<Damage>,
    ) -> Result<()> {
        if !(format == dom::Format::ARGB8888 || format == dom::Format::XRGB8888) {
            return Err(anyhow!("Invalid image format"));
        }

        let image = self.d_resource_thundr_image.get_mut(res).ok_or(anyhow!(
            "Resource does not have a internal GPU resource defined"
        ))?;

        self.d_thund
            .update_image_from_bits(&image, data, width, height, stride, damage, None)
            .context("Could not update image with damaged region")?;

        Ok(())
    }

    /// Populate a resource by importing a dmabuf
    ///
    /// This allows for loading the `fd` specified into Dakota's internal
    /// renderer without any copies. `modifier` must be supported by the
    /// Dakota device in use.
    pub fn define_resource_from_dmabuf(
        &mut self,
        res: &DakotaId,
        dmabuf: &Dmabuf,
        release_info: Option<Box<dyn Droppable + Send + Sync>>,
    ) -> Result<()> {
        if self.is_resource_defined(res) {
            return Err(anyhow!("Cannot redefine Resource contents"));
        }

        let image = self
            .d_thund
            .create_image_from_dmabuf(dmabuf, release_info)
            .context("Could not create Image resources")?;

        self.d_resource_thundr_image.set(res, image);
        Ok(())
    }

    /// Helper for populating an element with default formatting
    /// regular text. This saves the user from fully specifying the details
    /// of the text objects for this common operation.
    pub fn set_text_regular(&mut self, resource: &DakotaId, text: &str) {
        self.d_texts.set(
            resource,
            dom::Text {
                items: vec![dom::TextItem::p(dom::TextRun {
                    value: text.to_owned(),
                    cache: None,
                })],
            },
        );
    }

    /// Get the current size of the drawing region for this display
    pub fn get_resolution(&self) -> (u32, u32) {
        self.d_thund.get_resolution()
    }

    /// Get the major, minor of the DRM device currently in use
    pub fn get_drm_dev(&self) -> (i64, i64) {
        self.d_thund.get_drm_dev()
    }

    /// Get the total internal size for this layout node. This is used to calculate
    /// the scrolling region within this node, useful if it is a viewport node.
    fn get_node_internal_size(&self, id: DakotaId) -> (i32, i32) {
        let node = self.d_layout_nodes.get(&id).unwrap();
        let mut ret = (node.l_size.width, node.l_size.height);

        for child_id in node.l_children.iter() {
            let child = self.d_layout_nodes.get(&child_id).unwrap();

            // If this childs end position is larger, adjust our returning size
            // accordingly
            ret.0 = ret.0.max(child.l_offset.x + child.l_size.width);
            ret.1 = ret.1.max(child.l_offset.y + child.l_size.height);
        }

        return ret;
    }

    /// Fill in a new viewport entry for this layout node
    fn set_viewport(&self, id: &DakotaId) {
        let layout = self.d_layout_nodes.get(&id).unwrap();

        // Size and scroll offset will get updated elsewhere
        let mut viewport = th::Viewport::new(
            layout.l_offset.x as i32,
            layout.l_offset.y as i32,
            layout.l_size.width as i32,
            layout.l_size.height as i32,
        );
        let scroll_region = self.get_node_internal_size(id.clone());
        viewport.set_scroll_region(scroll_region.0 as i32, scroll_region.1 as i32);

        self.d_viewports.set(id, viewport);
    }

    fn assert_id_has_type(&self, id: &DakotaId, ty: DakotaObjectType) {
        let id_type = *self
            .d_node_types
            .get(id)
            .expect("Dakota node not assigned an object type");

        assert!(id_type == ty);
    }

    /// Add `child` as a child element to `parent`.
    ///
    /// This operation on makes sense for Dakota objects with the `Element` object
    /// type. Will only add `child` if it is not already a child of `parent`.
    pub fn add_child_to_element(&mut self, parent: &DakotaId, child: DakotaId) {
        // Assert this id has the Element type
        self.assert_id_has_type(parent, DakotaObjectType::Element);
        self.assert_id_has_type(&child, DakotaObjectType::Element);

        // Add old_id as a child element
        if self.d_children.get_mut(parent).is_none() {
            self.d_children.set(parent, Vec::new());
        }
        let mut children = self.d_children.get_mut(parent).unwrap();

        if children
            .iter()
            .find(|c| c.get_raw_id() == child.get_raw_id())
            .is_none()
        {
            children.push(child);
        }
    }

    /// Remove `child` as a child element of `parent`.
    ///
    /// This operation on makes sense for Dakota objects with the `Element` object
    /// type. This does nothing if `child` is not a child of `parent`.
    pub fn remove_child_from_element(&mut self, parent: &DakotaId, child: &DakotaId) -> Result<()> {
        // Assert this id has the Element type
        self.assert_id_has_type(parent, DakotaObjectType::Element);
        self.assert_id_has_type(&child, DakotaObjectType::Element);

        let mut children = match self.d_children.get_mut(parent) {
            Some(children) => children,
            None => return Ok(()),
        };

        // Get the indices of our two children
        if let Some(pos) = children
            .iter()
            .position(|c| c.get_raw_id() == child.get_raw_id())
        {
            children.remove(pos);
        }

        Ok(())
    }

    /// Reorder two elements that are children of parent
    ///
    /// Depending on the value of `order`, this will insert child A above or below
    /// child B in the element list.
    ///
    /// This is best used for when you need to bring an element to the front or back
    /// of a child list without regenerating the entire thing. This is particularly
    /// useful for category5, which orders elements for wayland subsurfaces
    pub fn reorder_children_element(
        &mut self,
        parent: &DakotaId,
        order: SubsurfaceOrder,
        a: &DakotaId,
        b: &DakotaId,
    ) -> Result<()> {
        // Assert this id has the Element type
        self.assert_id_has_type(parent, DakotaObjectType::Element);
        self.assert_id_has_type(a, DakotaObjectType::Element);
        self.assert_id_has_type(b, DakotaObjectType::Element);

        let mut children = self
            .d_children
            .get_mut(parent)
            .context("Parent does not have any children, cannot reorder")?;

        // Get the indices of our two children
        let pos_a = children
            .iter()
            .position(|c| c.get_raw_id() == a.get_raw_id())
            .context("Could not find Child A in Parent's children")?;
        let pos_b = children
            .iter()
            .position(|c| c.get_raw_id() == b.get_raw_id())
            .context("Could not find Child B in Parent's children")?;

        // Remove child A and insert it above or below child B
        children.remove(pos_a);
        children.insert(
            match order {
                SubsurfaceOrder::Above => pos_b + 1,
                SubsurfaceOrder::Below => pos_b,
            },
            a.clone(),
        );

        Ok(())
    }

    /// Move child to front of children in parent
    ///
    /// This is used for bringing an element into "focus", and placing it as
    /// the foremost child.
    pub fn move_child_to_front(&mut self, parent: &DakotaId, child: &DakotaId) -> Result<()> {
        // Assert this id has the Element type
        self.assert_id_has_type(parent, DakotaObjectType::Element);
        self.assert_id_has_type(child, DakotaObjectType::Element);

        let mut children = self
            .d_children
            .get_mut(parent)
            .context("Parent does not have any children, cannot reorder")?;

        // Get the indices of our two children
        let pos = children
            .iter()
            .position(|c| c.get_raw_id() == child.get_raw_id())
            .context("Could not find Child A in Parent's children")?;

        // Remove child A and insert it above or below child B
        children.remove(pos);
        children.push(child.clone());

        Ok(())
    }

    /// Set the resolution of the current window
    pub fn set_resolution(&mut self, dom_id: &DakotaId, width: u32, height: u32) -> Result<()> {
        let dom = self
            .d_dom
            .get(dom_id)
            .ok_or(anyhow!("Only DOM objects can be refreshed"))?;
        self.d_plat
            .set_output_params(&dom.window, (width, height))?;

        Ok(())
    }

    /// This refreshes the entire scene, and regenerates
    /// the Thundr surface list.
    pub fn refresh_elements(&mut self, dom_id: &DakotaId) -> Result<()> {
        log::verbose!("Dakota: Refreshing element tree");
        let root_node_id = {
            let dom = self
                .d_dom
                .get(dom_id)
                .ok_or(anyhow!("Only DOM objects can be refreshed"))?;

            // check if the window size is set. If it is not, this is the
            // first iteration and we need to populate the dimensions
            // from the DOM
            if self.d_window_dims.is_none() {
                // If the user specified a window size use that, otherwise
                // use the current vulkan surface size.
                //
                // This is important for physical display presentation, where
                // we want to grow to the size of the screen unless told otherwise.
                if let Some(size) = dom.window.size.as_ref() {
                    self.d_window_dims = Some((size.0, size.1));
                } else {
                    self.d_window_dims = Some(self.d_thund.get_resolution());
                }

                // we need to update the window dimensions if possible,
                // so call into our platform do handle it
                self.d_plat
                    .set_output_params(&dom.window, self.d_window_dims.unwrap())?;
            }
            dom.root_element.clone()
        };

        // Set the size of our root node. We need to assign this a size manually so
        // that it doesn't default and size itself to its children, causing the viewport
        // scroll region calculation to go wrong.
        let resolution = self.get_resolution();
        self.d_widths
            .set(&root_node_id, dom::Value::Constant(resolution.0 as i32));
        self.d_heights
            .set(&root_node_id, dom::Value::Constant(resolution.1 as i32));

        // Reset our old tree
        self.d_layout_tree_root = None;

        // construct layout tree with sizes of all boxes
        self.calculate_sizes(
            &root_node_id,
            None, // no parent since we are the root node
            &LayoutSpace {
                avail_width: self.d_window_dims.unwrap().0 as i32, // available width
                avail_height: self.d_window_dims.unwrap().1 as i32, // available height
            },
        )?;
        // Manually mark the root node as a viewport node. It always is, and it will
        // always have the root viewport.
        self.set_viewport(&root_node_id);

        // Perform the Thundr pass
        //
        self.d_layout_tree_root = Some(root_node_id);
        self.d_needs_redraw = true;
        self.clear_needs_refresh();

        Ok(())
    }

    /// Completely flush the thundr surfaces/images and recreate the scene
    pub fn refresh_full(&mut self, dom: &DakotaId) -> Result<()> {
        self.refresh_elements(dom)
            .context("Refreshing element layout")
    }

    /// Handle vulkan swapchain out of date. This is probably because the
    /// window's size has changed. This will requery the window size and
    /// refresh the layout tree.
    fn handle_ood(&mut self, dom_id: &DakotaId) -> Result<()> {
        let new_res = self.d_thund.get_resolution();
        let dom = self
            .d_dom
            .get(dom_id)
            .ok_or(anyhow!("Only DOM objects can be refreshed"))?;

        self.d_event_sys.add_event_window_resized(
            dom.deref(),
            dom::Size {
                width: new_res.0,
                height: new_res.1,
            },
        );

        self.d_needs_redraw = true;
        self.d_needs_refresh = true;
        self.d_ood_counter = 30;
        self.d_window_dims = Some(new_res);
        Ok(())
    }

    /// Get the slice of currently unhandled events
    ///
    /// The app should do this in its main loop after dispatching.
    /// These will be cleared during each dispatch.
    pub fn drain_events<'b>(&'b mut self) -> std::collections::vec_deque::Drain<'b, Event> {
        self.d_event_sys.drain_events()
    }

    fn viewport_at_pos_recursive(
        &self,
        id: &DakotaId,
        base: (i32, i32),
        x: i32,
        y: i32,
    ) -> Option<DakotaId> {
        let layout = self.d_layout_nodes.get(id).unwrap();
        let offset = (base.0 + layout.l_offset.x, base.1 + layout.l_offset.y);

        // Since the tree is back to front, process the children first. If one of them is a match,
        // it is the top-most viewport and we should return it. Otherwise we can test if this
        // node matches
        for child in layout.l_children.iter() {
            if let Some(ret) = self.viewport_at_pos_recursive(child, offset, x, y) {
                return Some(ret);
            }
        }

        // If this node is not a viewport return nothing
        if self.d_viewports.get(id).is_none() {
            return None;
        }

        let x_range = offset.0..(offset.0 + layout.l_size.width);
        let y_range = offset.1..(offset.1 + layout.l_size.height);

        if x_range.contains(&x) && y_range.contains(&y) {
            return Some(id.clone());
        }

        None
    }

    /// Walks the viewport tree and returns the ECS id of the
    /// viewport at this location. Note there will always be a viewport
    /// because the entire window surface is at the very least, the root viewport
    fn viewport_at_pos(&self, x: i32, y: i32) -> DakotaId {
        assert!(self.d_layout_tree_root.is_some());
        let root_node = self.d_layout_tree_root.as_ref().unwrap();
        assert!(self.d_viewports.get(root_node).is_some());

        self.viewport_at_pos_recursive(root_node, (0, 0), x, y)
            .unwrap()
    }

    /// Handle dakota-only events coming from the event system
    ///
    /// Most notably this handles scrolling
    fn handle_private_events(&mut self) -> Result<()> {
        for i in 0..self.d_event_sys.es_dakota_event_queue.len() {
            let ev = &self.d_event_sys.es_dakota_event_queue[i];
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
                    let node = self.viewport_at_pos(self.d_mouse_pos.0, self.d_mouse_pos.1);
                    let mut viewport = self.d_viewports.get_mut(&node).unwrap();
                    log::error!("original_scroll_offset: {:?}", viewport.scroll_offset);

                    viewport.update_scroll_amount(x, y);
                    log::error!("new_scroll_offset: {:?}", viewport.scroll_offset);

                    self.d_needs_redraw = true;
                }
                // Ignore all other events for now
                _ => {}
            }
        }

        self.d_event_sys.es_dakota_event_queue.clear();
        Ok(())
    }

    /// Add a file descriptor to watch
    ///
    /// This will add a new file descriptor to the watch set inside dakota,
    /// meaning dakota will return control to the user when this fd is readable.
    /// This is done through the `UserFdReadable` event.
    pub fn add_watch_fd(&mut self, fd: RawFd) {
        self.d_plat.add_watch_fd(fd);
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
    pub fn dispatch(&mut self, dom: &DakotaId, mut timeout: Option<usize>) -> Result<()> {
        let mut first_loop = true;

        loop {
            if !first_loop || self.d_ood_counter > 0 {
                timeout = Some(0);
                self.d_ood_counter -= 1;
                self.d_needs_redraw = true;
            }
            first_loop = false;

            // First handle input and platform changes
            match self.dispatch_platform(dom, timeout) {
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
            match self.dispatch_rendering(dom) {
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
    pub fn dispatch_platform(&mut self, dom: &DakotaId, timeout: Option<usize>) -> Result<()> {
        // First run our window system code. This will check if wayland/X11
        // notified us of a resize, closure, or need to redraw
        let plat_res = self.d_plat.run(
            &mut self.d_event_sys,
            self.d_dom
                .get(dom)
                .ok_or(anyhow!("Id passed to Dispatch must be a DOM object"))?
                .deref(),
            timeout,
        );

        match plat_res {
            Ok(needs_redraw) => {
                if needs_redraw {
                    self.d_needs_redraw = needs_redraw
                }
            }
            Err(th::ThundrError::OUT_OF_DATE) => {
                // This is a weird one
                // So the above OUT_OF_DATEs are returned from thundr, where we
                // can expect it will handle OOD itself. But here we have
                // OUT_OF_DATE returned from our SDL2 backend, so we need
                // to tell Thundr to do OOD itself
                self.d_thund.handle_ood()?;
                self.handle_ood(dom)?;
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
    pub fn dispatch_rendering(&mut self, dom: &DakotaId) -> Result<()> {
        let mut stop = StopWatch::new();

        // Now handle events like scrolling before we calculate sizes
        self.handle_private_events()?;

        if self.needs_refresh() {
            let mut layout_stop = StopWatch::new();
            layout_stop.start();
            self.refresh_elements(dom)?;
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
        if self.d_needs_redraw {
            match self.draw_surfacelists() {
                Ok(()) => {}
                Err(th::ThundrError::OUT_OF_DATE) => {
                    self.handle_ood(dom)?;
                    return Err(th::ThundrError::OUT_OF_DATE.into());
                }
                Err(e) => return Err(Error::from(e).context("Thundr: drawing failed with error")),
            };
            match self.d_thund.present() {
                Ok(()) => {}
                Err(th::ThundrError::OUT_OF_DATE) => {
                    self.handle_ood(dom)?;
                    return Err(th::ThundrError::OUT_OF_DATE.into());
                }
                Err(e) => return Err(Error::from(e).context("Thundr: presentation failed")),
            };
            self.d_needs_redraw = false;

            // Notify the app that we just drew a frame and it should prepare the next one
            self.d_event_sys.add_event_window_redraw_complete(
                self.d_dom
                    .get(dom)
                    .ok_or(anyhow!("Id passed to Dispatch must be a DOM object"))?
                    .deref(),
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
    pub fn dump_framebuffer(&mut self, filename: &str) -> MappedImage {
        self.d_thund.dump_framebuffer(filename)
    }
}
