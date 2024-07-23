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

mod font;
use font::*;

// Re-exmport our getters/setters
mod generated;

use std::ops::Deref;
use std::ops::DerefMut;
use std::os::fd::RawFd;
extern crate regex;
use regex::Regex;

fn regex_trim_excess_space(str: &String) -> String {
    let re = Regex::new(r"\s+").unwrap();
    let trimmed = re.replace_all(str, " ");
    trimmed.to_string()
}

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

/// The elements of the layout tree.
/// This will be constructed from the Elements in the DOM
#[derive(Debug)]
pub(crate) struct LayoutNode {
    /// Is this element a glyph subsurface. If so it is one character
    /// in a block of text. This is really an index into the font.
    pub l_glyph_id: Option<u16>,
    /// True if the dakota file specified an offset for this el
    pub l_offset_specified: bool,
    pub l_offset: dom::Offset<i32>,
    pub l_size: dom::Size<i32>,
    /// Ids of the children that this layout node has
    pub l_children: Vec<DakotaId>,
}

impl Default for LayoutNode {
    fn default() -> Self {
        Self {
            l_glyph_id: None,
            l_offset_specified: false,
            l_offset: dom::Offset::new(0, 0),
            l_size: dom::Size::new(0, 0),
            l_children: Vec::with_capacity(0),
        }
    }
}

impl LayoutNode {
    fn new(glyph_id: Option<u16>, off: dom::Offset<i32>, size: dom::Size<i32>) -> Self {
        Self {
            l_glyph_id: glyph_id,
            l_offset_specified: false,
            l_offset: off,
            l_size: size,
            l_children: Vec::with_capacity(0),
        }
    }

    fn add_child(&mut self, other: ll::Entity) {
        self.l_children.push(other);
    }

    /// Resize this element to contain all of its children.
    ///
    /// This can be used when the size of a box was not specified, and it
    /// should be grown to be able to hold all of the child boxes.
    ///
    /// We don't need to worry about bounding by an available size, this is
    /// to be used when there are no bounds (such as in a scrolling arena) and
    /// we just want to grow this element to fit everything.
    #[allow(dead_code)]
    pub fn resize_to_children(&mut self, dakota: &Dakota) -> Result<()> {
        self.l_size = dom::Size {
            width: 0,
            height: 0,
        };

        for child_id in self.l_children.iter() {
            let other = &dakota.d_layout_nodes.get(child_id).unwrap();

            // add any offsets to our size
            self.l_size.width += other.l_offset.x + other.l_size.width;
            self.l_size.height += other.l_offset.y + other.l_size.height;
        }

        return Ok(());
    }
}

/// Used for tracking layout of children
struct TileInfo {
    /// The latest position we have marched horizontally
    /// while laying children.
    t_last_x: u32,
    /// Same as last width, but in the Y axis.
    t_last_y: u32,
    /// The last known greatest height. This is what the next
    /// height will be when a line overflows.
    t_greatest_y: u32,
}

/// This is the available space for a layout calculation.
/// this handles the number of children sharing the space, the
/// available size
#[derive(Debug, Clone)]
pub struct LayoutSpace {
    /// This is essentially the width of the parent container
    pub avail_width: i32,
    /// This is essentially the height of the parent container
    pub avail_height: i32,
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

    /// Calculate size and position of centered content.
    ///
    ///
    /// This box has centered content.
    /// We should either recurse the child box or calculate the
    /// size based on the centered resource.
    fn calculate_sizes_content(&mut self, el: &DakotaId, space: &LayoutSpace) -> Result<()> {
        log::debug!("Calculating content size");
        let child_id = self.d_contents.get(el).unwrap().el.clone();

        self.calculate_sizes(&child_id, Some(el), &space)?;
        let parent_size = self.d_layout_nodes.get(el).unwrap().l_size;

        {
            let mut child_size_raw = self.d_layout_nodes.get_mut(&child_id).unwrap();
            let child_size = child_size_raw.deref_mut();
            // At this point the size of the is calculated
            // and we can determine the offset. We want to center the
            // box, so that's the center point of the parent minus
            // half the size of the child.
            //
            // The child size should have already been clipped to the available space
            child_size.l_offset.x =
                utils::partial_max((parent_size.width / 2) - (child_size.l_size.width / 2), 0);
            child_size.l_offset.y =
                utils::partial_max((parent_size.height / 2) - (child_size.l_size.height / 2), 0);
        }

        let mut node = self.d_layout_nodes.get_mut(el).unwrap();
        node.add_child(child_id.clone());
        Ok(())
    }

    /// Recursively calls calculate_sizes on all children of el
    ///
    /// This does all the work to get information about children for a particular
    /// element. After having the children calculate their sizes, it will assign
    /// them layout positions within el. This will fill from left to right by
    /// default, wrapping below if necessary.
    ///
    /// `grandparent` is avialable when appropriate and allows children to
    /// reference two levels above, for use when not bounding size by the
    /// current element.
    fn calculate_sizes_children(&mut self, el: &DakotaId, space: &mut LayoutSpace) -> Result<()> {
        log::debug!("Calculating children size");
        // TODO: do vertical wrapping too
        let mut tile_info = TileInfo {
            t_last_x: 0,
            t_last_y: 0,
            t_greatest_y: 0,
        };

        let child_count = self
            .d_children
            .get(el)
            .ok_or(anyhow!("Expected children"))?
            .len();

        for i in 0..child_count {
            let child_id = self
                .d_children
                .get(el)
                .ok_or(anyhow!("Expected children"))?[i]
                .clone();
            self.calculate_sizes(&child_id, Some(el), &space)?;

            // ----- adjust child position ----
            {
                let mut child_size = self.d_layout_nodes.get_mut(&child_id).unwrap();

                // now the child size has been made, but it still needs to find
                // the proper position inside the parent container. If the child
                // already had an offset specified, it is "out of the loop", and
                // doesn't get used for pretty formatting, it just gets placed
                // wherever.
                if !child_size.l_offset_specified {
                    // if this element exceeds the horizontal or vertical space, set it on a
                    // new line
                    if tile_info.t_last_x as i32 + child_size.l_size.width > space.avail_width
                        || tile_info.t_last_y as i32 + child_size.l_size.height > space.avail_height
                    {
                        tile_info.t_last_x = 0;
                        tile_info.t_last_y = tile_info.t_greatest_y;
                    }

                    child_size.l_offset = dom::Offset {
                        x: tile_info.t_last_x as i32,
                        y: tile_info.t_last_y as i32,
                    };

                    // now we need to update the space that we have seen children
                    // occupy, so we know where to place the next children in the
                    // tiling formation.
                    tile_info.t_last_x += child_size.l_size.width as u32;
                    tile_info.t_greatest_y = std::cmp::max(
                        tile_info.t_greatest_y,
                        tile_info.t_last_y + child_size.l_size.height as u32,
                    );
                }
            }

            self.d_layout_nodes
                .get_mut(el)
                .unwrap()
                .add_child(child_id.clone());
        }

        Ok(())
    }

    /// Calculate the sizes and handle the current element
    ///
    /// 1. If it has a size assigned, that is the final size, all children
    /// will be clipped or scrolled inside that window.
    /// 2. If no size is assigned, and we are limited in the amount of space
    /// we have, then the size is available_space
    /// 3. No size and no bounds means we are inside of a scrolling arena, and
    /// we should grow this box to hold all of its children.
    ///
    /// The final size of this element will be amended after all child content
    /// has been calculated.
    fn calculate_sizes_el(
        &mut self,
        el: &DakotaId,
        _parent: Option<&DakotaId>,
        space: &LayoutSpace,
    ) -> Result<()> {
        let mut node = LayoutNode::new(None, dom::Offset::new(0, 0), dom::Size::new(0, 0));

        node.l_offset_specified = self.d_offsets.get(el).is_some();
        node.l_offset = self
            .get_final_offset(el, &space)
            .context("Failed to calculate offset size of Element")?
            .into();

        node.l_size = self.get_default_size(el, space)?.into();
        // Bounds will be checked in caller to see if the parent needs to be
        // marked as a viewport.

        log::debug!("Offset of element is {:?}", node.l_offset);
        log::debug!("Size of element is {:?}", node.l_size);
        self.d_layout_nodes.take(el);
        self.d_layout_nodes.set(el, node);
        Ok(())
    }

    /// Helper to get the Font Instance for a particular element
    ///
    /// This will choose the default font (including size) if none
    /// has been assigned.
    fn get_font_id_for_el(&self, el: &DakotaId) -> DakotaId {
        match self.d_text_font.get(el) {
            Some(f) => f.clone(),
            None => self.d_default_font_inst.clone(),
        }
    }

    /// Handles creating LayoutNodes for every glyph in a passage
    ///
    /// This is the handler for the text field in the dakota file
    fn calculate_sizes_text(&mut self, el: &DakotaId) -> Result<()> {
        let font_id = self.get_font_id_for_el(el);
        let font = self.d_fonts.get(&font_id).unwrap();
        let font_inst = &mut self
            .d_font_instances
            .iter_mut()
            .find(|(f, _)| *f == *font)
            .expect("Could not find FontInstance")
            .1;

        let mut text = self.d_texts.get_mut(el).unwrap();
        let line_space = font_inst.get_vertical_line_spacing();

        // This is how far we have advanced on a line
        // Go down by one line space before writing the first line. This deals
        // with the problem that ft/hb want to index by the bottom left corner
        // and all my stuff wants to index from the top left corner. Without this
        // text starts being written "above" the element it is assigned to.
        let mut cursor = {
            let node = self.d_layout_nodes.get(el).unwrap();
            Cursor {
                c_i: 0,
                c_x: 0,
                c_y: line_space,
                c_min: node.l_offset.x,
                c_max: node.l_offset.x + node.l_size.width,
            }
        };

        log::debug!("Calculating text size");
        log::debug!("{:?}", cursor);

        // Trim out newlines and tabs. Styling is done with entries in the DOM, not
        // through text formatting in the dakota file.
        for item in text.items.iter_mut() {
            match item {
                dom::TextItem::p(run) | dom::TextItem::b(run) => {
                    // We need to take references to everything at once before the closure
                    // so that the borrow checker can see we aren't trying to reference all
                    // of self
                    let layouts = &mut self.d_layout_nodes;
                    let text_fonts = &mut self.d_text_font;

                    if run.cache.is_none() {
                        // TODO: we can get the available height from above, pass it to a font instance
                        // and create layout nodes for all character surfaces.
                        let mut trim = regex_trim_excess_space(&run.value);
                        // TODO: Find a better way of adding space around itemized runs
                        trim.push_str(" ");

                        run.cache = Some(font_inst.initialize_cached_chars(
                            &mut self.d_thund,
                            &mut self.d_ecs_inst,
                            &trim,
                        ));
                    }

                    // Record text locations
                    // We will create a whole bunch of sub-nodes which will be assigned
                    // glyph ids. These ids will later be used to get surfaces for.
                    font_inst.layout_text(
                        &mut self.d_thund,
                        &mut cursor,
                        run.cache.as_ref().unwrap(),
                        &mut |inst: &mut FontInstance, thund, curse, ch| {
                            // --- calculate sizes for the character surfaces ---
                            let size = inst.get_glyph_thundr_size(thund, ch.glyph_id);

                            let child_size = LayoutNode::new(
                                Some(ch.glyph_id),
                                dom::Offset {
                                    x: curse.c_x + ch.offset.0,
                                    y: curse.c_y + ch.offset.1,
                                },
                                dom::Size {
                                    width: size.0,
                                    height: size.1,
                                },
                            );
                            log::info!("Character size is {:?}", size);

                            {
                                let mut node = layouts.get_mut(el).unwrap();
                                // What we have done here is create a "fake" element (fake since
                                // the user didn't specify it) that represents a glyph.
                                node.add_child(ch.node.clone());
                            }

                            layouts.take(&ch.node);
                            layouts.set(&ch.node, child_size);
                            // We need to assign a font here or else later when we
                            // create thundr surfaces for these glyphs we will index
                            // the wrong font using this glyph_id
                            text_fonts.set(&ch.node, font_id.clone());
                        },
                    );
                }
            }
        }

        Ok(())
    }

    /// Create a layout tree of boxes.
    ///
    /// This gives all the layout information for where we should place
    /// thundr surfaces.
    ///
    /// This will add boxes to the box array, but will also return the
    /// box signifying the final size. By handing the size up the recursion
    /// chain, each box can see the sizes of its children as they are
    /// created, and can set its final size accordingly. This should prevent
    /// us from having to do more recursion later since everything is calculated
    /// now.
    fn calculate_sizes(
        &mut self,
        el: &DakotaId,
        parent: Option<&DakotaId>,
        space: &LayoutSpace,
    ) -> Result<()> {
        // ------------------------------------------
        // HANDLE THIS ELEMENT
        // ------------------------------------------
        // Must be done before anything referencing the size of this element
        self.calculate_sizes_el(el, parent, space)
            .context("Layout Tree Calculation: processing element")?;

        // This space is what the children/content will use
        // it is restricted in size to this element (their parent)
        let mut child_space = {
            let node = self.d_layout_nodes.get(el).unwrap();
            LayoutSpace {
                avail_width: node.l_size.width,
                avail_height: node.l_size.height,
            }
        };

        // ------------------------------------------
        // HANDLE TEXT
        // ------------------------------------------
        // We do this after handling the size of the current element so that we
        // can know what width we have available to fill in with text.
        if self.d_texts.get(el).is_some() {
            self.calculate_sizes_text(el)?;
        }

        // if the box has children, then recurse through them and calculate our
        // box size based on the fill type.
        if self.d_children.get(el).is_some() && self.d_children.get(el).unwrap().len() > 0 {
            // ------------------------------------------
            // CHILDREN
            // ------------------------------------------
            //

            self.calculate_sizes_children(el, &mut child_space)
                .context("Layout Tree Calculation: processing children of element")?;
        }

        if self.d_contents.get(el).is_some() {
            // ------------------------------------------
            // CENTERED CONTENT
            // ------------------------------------------
            self.calculate_sizes_content(el, &child_space)
                .context("Layout Tree Calculation: processing centered content of element")?;
        }

        // Update the size of this element after calculating the content
        let final_size = self.get_final_size(el, space)?.into();
        self.d_layout_nodes.get_mut(el).unwrap().l_size = final_size;

        return Ok(());
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

    /// Populate a thundr surface with this nodes dimensions and content
    ///
    /// This accepts a base offset to handle child element positioning
    fn get_thundr_surf_for_el(
        &mut self,
        node: &DakotaId,
        base: (i32, i32),
    ) -> th::Result<th::Surface> {
        let layout = self.d_layout_nodes.get(node).unwrap();

        // If this node is a viewport then ignore its offset, setting the viewport
        // will take care of positioning it.
        let offset = match self.d_viewports.get(node).is_some() {
            true => (0, 0),
            false => (base.0 + layout.l_offset.x, base.1 + layout.l_offset.y),
        };

        // Image/color content will be set later
        let mut surf = if let Some(glyph_id) = layout.l_glyph_id {
            let font_id = self.get_font_id_for_el(node);
            let font = self.d_fonts.get(&font_id).unwrap();
            let font_inst = &mut self
                .d_font_instances
                .iter_mut()
                .find(|(f, _)| *f == *font)
                .expect("Could not find FontInstance")
                .1;
            // If this path is hit, then this layout node is really a glyph in a
            // larger block of text. It has been created as a child, and isn't
            // a real element. We ask the font code to give us a surface for
            // it that we can display.
            font_inst.get_thundr_surf_for_glyph(&mut self.d_thund, glyph_id, &offset)
        } else {
            th::Surface::new(
                th::Rect::new(
                    offset.0,
                    offset.1,
                    layout.l_size.width,
                    layout.l_size.height,
                ),
                None, // image
                None, // color
            )
        };

        // Handle binding images
        // We need to get the resource's content from our resource map, get
        // the thundr image for it, and bind it to our new surface.
        if let Some(resource_id) = self.d_resources.get(node) {
            // Assert that only one content type is set
            let mut content_num = 0;

            if let Some(image) = self.d_resource_thundr_image.get(&resource_id) {
                surf.bind_image(image.clone());
                content_num += 1;
            }
            if let Some(color) = self.d_resource_color.get(&resource_id) {
                surf.set_color((color.r, color.g, color.b, color.a));
                content_num += 1;
            }

            assert!(content_num == 1);
        }

        return Ok(surf);
    }

    /// Create a Thundr viewport struct from our dakota Viewport
    ///
    /// This would be straightforward except that we have to clip our viewport
    /// to the size of the parent viewport. This keeps child elements within the
    /// bounds of the parent.
    fn get_thundr_viewport(
        &self,
        parent: &th::Viewport,
        node: &DakotaId, // child viewport
        base: (i32, i32),
    ) -> Option<th::Viewport> {
        let layout = self.d_layout_nodes.get(node)?;
        let viewport = self.d_viewports.get(node)?;

        // We will copy the viewport, and then return a clamped version of it to
        // draw with.
        let mut ret = viewport.clone();

        // If the child is partially scrolled past, then update its offset to
        // zero and limit the size by that amount
        let clamp_to_parent_base = |child_original_size,
                                    child_offset: &mut i32,
                                    child_size: &mut i32,
                                    parent_offset: i32,
                                    parent_size: i32| {
            // The child size is either size reduced by the amount this
            // child is behind the parent, or the size reduced by the amount
            // this child exceeds the parent, or the size
            *child_size = if *child_offset < parent_offset {
                child_original_size - (parent_offset - *child_offset).abs()
            } else if *child_offset + child_original_size > parent_offset + parent_size {
                (parent_offset + parent_size) - *child_offset
            } else {
                child_original_size
            };
            // Now clamp it to the parent's region
            *child_offset = (*child_offset).clamp(parent_offset, parent_offset + parent_size);
        };

        // Update the starting dimensions of the returned viewport
        ret.offset = (
            base.0 as i32 + layout.l_offset.x as i32,
            base.1 as i32 + layout.l_offset.y as i32,
        );
        ret.size = (layout.l_size.width as i32, layout.l_size.height as i32);

        // Clamp it to the parent
        clamp_to_parent_base(
            layout.l_size.width as i32,
            &mut ret.offset.0,
            &mut ret.size.0,
            parent.offset.0,
            parent.size.0,
        );
        clamp_to_parent_base(
            layout.l_size.height as i32,
            &mut ret.offset.1,
            &mut ret.size.1,
            parent.offset.1,
            parent.size.1,
        );

        return Some(ret);
    }

    /// Helper for drawing a single element
    ///
    /// This does not recurse. Will skip drawing this node if it is out of the bounds of
    /// its viewport.
    fn draw_node(
        &mut self,
        viewport: &th::Viewport,
        node: &DakotaId,
        base: (i32, i32),
    ) -> th::Result<()> {
        {
            let layout = self.d_layout_nodes.get(node).unwrap();

            // Test that this child is visible before drawing it
            let offset = dom::Offset::new(base.0 + layout.l_offset.x, base.1 + layout.l_offset.y);
            if (offset.x > viewport.size.0
                    && offset.y > viewport.size.1 )
                    // Have we scrolled past this horizontally
                    || (offset.x < 0 && offset.x * -1 > layout.l_size.width)
                    // Have we scrolled past this vertically
                    || (offset.y < 0 && offset.y * -1 > layout.l_size.height)
            {
                return Ok(());
            }
        }

        let surf = self.get_thundr_surf_for_el(node, base)?;

        self.d_thund.draw_surface(&surf)
    }

    /// Recursively draw node and all of its children
    ///
    /// This does not cross viewport boundaries
    fn draw_node_recurse(
        &mut self,
        viewport: &th::Viewport,
        node: &DakotaId,
        base: (i32, i32),
    ) -> th::Result<()> {
        // If this node is a viewport then update our thundr viewport
        let new_th_viewport = match self.d_viewports.get(node).is_some() {
            true => {
                // Set Thundr's currently in use viewport
                let th_viewport = self.get_thundr_viewport(viewport, node, base).unwrap();
                self.d_thund.set_viewport(&th_viewport)?;

                Some(th_viewport)
            }
            false => None,
        };

        let new_viewport = match self.d_viewports.get(node).is_some() {
            true => new_th_viewport.as_ref().unwrap(),
            false => viewport,
        };

        // Start by drawing ourselves
        self.draw_node(new_viewport, node, base)?;

        // Help borrow checker. We don't do any modification to this while
        // drawing so it's fine.
        let layout_nodes = self.d_layout_nodes.clone();
        let layout = layout_nodes.get(node).unwrap();

        // Update our subsurf offset
        // If this node is a viewport then the base offset needs to be reset
        let new_base = match self.d_viewports.get(node) {
            // do scrolling here to allow us to test things that are off screen?
            // By putting the offset here all children will be offset by it
            Some(vp) => (vp.scroll_offset.0, vp.scroll_offset.1),
            None => (base.0 + layout.l_offset.x, base.1 + layout.l_offset.y),
        };

        // Now draw each of our children
        for child in layout.l_children.iter() {
            self.draw_node_recurse(new_viewport, child, new_base)?;
        }

        // If this node was a viewport then restore our old viewport
        if new_th_viewport.is_some() {
            self.d_thund.set_viewport(viewport)?;
        }

        Ok(())
    }

    /// Draw the entire scene
    ///
    /// This starts at the root viewport and draws all child viewports
    fn draw_surfacelists(&mut self) -> th::Result<()> {
        let root_node = self.d_layout_tree_root.clone().unwrap();
        let root_viewport = self.d_viewports.get_clone(&root_node).unwrap();

        self.d_thund.begin_recording()?;
        self.draw_node_recurse(&root_viewport, &root_node, (0, 0))?;
        self.d_thund.end_recording()?;

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
