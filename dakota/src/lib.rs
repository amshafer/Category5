/// Dakota UI Toolkit
///
/// Dakota is a UI toolkit designed for rendering trees of surfaces. These
/// surfaces can be easily expressed in XML documents, and updated dynamically
/// by the application.
///
/// Austin Shafer - 2022
extern crate image;
extern crate lluvia as ll;
extern crate thundr as th;
pub use th::ThundrError as DakotaError;

extern crate bitflags;

extern crate lazy_static;
extern crate utils;
use utils::log;
pub use utils::{
    anyhow, fdwatch::FdWatch, region::Rect, timing::StopWatch, Context, Error, MemImage, Result,
};

pub mod dom;
pub mod input;
mod platform;
use platform::Platform;
pub mod xml;

pub mod event;
use event::{Event, EventSystem};

mod font;
use font::*;

// Re-exmport our getters/setters
mod generated;
pub use generated::*;

use std::ops::Deref;
use std::os::fd::RawFd;
extern crate regex;
use regex::Regex;

fn regex_trim_excess_space(str: &String) -> String {
    let re = Regex::new(r"\s+").unwrap();
    let trimmed = re.replace_all(str, " ");
    trimmed.to_string()
}

struct ResMapEntry {
    rme_image: Option<th::Image>,
    rme_color: Option<dom::Color>,
}

pub type DakotaId = ll::Entity;
// Since there are significantly fewer viewports we will give them
// their own ECS system so we don't waste space.
pub type ViewportId = ll::Entity;

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum DakotaObjectType {
    Element,
    DakotaDOM,
    Resource,
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
pub struct Dakota<'a> {
    // GROSS: we need thund to be before plat so that it gets dropped first
    // It might reference the window inside plat, and will segfault if
    // dropped after it.
    d_thund: th::Thundr,
    #[cfg(feature = "wayland")]
    d_plat: platform::WlPlat,
    #[cfg(feature = "sdl")]
    d_plat: platform::SDL2Plat,
    /// A set of fds provided by the application that we should watch during
    /// our main loop
    d_user_fds: Option<FdWatch>,
    /// This is one ECS that is composed of multiple tables
    d_ecs_inst: ll::Instance,
    /// This is all of the LayoutNodes in the system, each corresponding to
    /// an Element or a subcomponent of an Element. Indexed by DakotaId.
    d_layout_nodes: ll::Session<LayoutNode>,
    // NOTE: --------------------------------
    //
    // If you update the following you may have to edit the generated
    // getters/setters in generated.rs
    d_node_types: ll::Session<DakotaObjectType>,

    // Resource components
    // --------------------------------------------
    /// The resource's thundr data
    d_resource_entries: ll::Session<ResMapEntry>,
    /// The resource info configured by the user
    d_resource_definitions: ll::Session<dom::Resource>,

    // Element components
    // --------------------------------------------
    /// The resource currently assigned to this element
    d_resources: ll::Session<DakotaId>,
    d_offsets: ll::Session<dom::RelativeOffset>,
    d_sizes: ll::Session<dom::RelativeSize>,
    d_texts: ll::Session<dom::Text>,
    d_contents: ll::Session<dom::Content>,
    d_bounds: ll::Session<dom::Edges>,
    d_children: ll::Session<Vec<DakotaId>>,
    /// This is the corresponding thundr surface for each LayoutNode. Also
    /// indexed by DakotaId.
    d_layout_node_surfaces: ll::Session<th::Surface>,

    // DOM components
    // --------------------------------------------
    d_dom: ll::Session<dom::DakotaDOM>,

    d_viewport_ecs_inst: ll::Instance,
    d_viewport_nodes: ll::Session<ViewportNode>,
    /// This is the root node in the scene tree
    d_layout_tree_root: Option<DakotaId>,
    d_root_viewport: Option<ViewportId>,
    d_window_dims: Option<(u32, u32)>,
    d_needs_redraw: bool,
    d_needs_refresh: bool,
    d_event_sys: EventSystem,
    d_font_inst: FontInstance<'a>,
    d_ood_counter: usize,
}

struct ViewportNode {
    v_children: Vec<ViewportId>,
    v_root_node: Option<DakotaId>,
    v_viewport: th::Viewport,
    v_surfaces: th::SurfaceList,
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
    pub l_offset: dom::Offset<f32>,
    pub l_size: dom::Size<f32>,
    /// Ids of the children that this layout node has
    pub l_children: Vec<DakotaId>,
    /// Is this node a viewport boundary.
    ///
    /// This signifies that this node's children are larger than the node
    /// itself, and this node is a scrolling region. if this is true the
    /// associated viewport is the handler for this node.
    pub l_is_viewport: bool,
}

impl Default for LayoutNode {
    fn default() -> Self {
        Self {
            l_glyph_id: None,
            l_offset_specified: false,
            l_offset: dom::Offset::new(0.0, 0.0),
            l_size: dom::Size::new(0.0, 0.0),
            l_children: Vec::with_capacity(0),
            l_is_viewport: false,
        }
    }
}

impl LayoutNode {
    fn new(glyph_id: Option<u16>, off: dom::Offset<f32>, size: dom::Size<f32>) -> Self {
        Self {
            l_glyph_id: glyph_id,
            l_offset_specified: false,
            l_offset: off,
            l_size: size,
            l_children: Vec::with_capacity(0),
            l_is_viewport: false,
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
            width: 0.0,
            height: 0.0,
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
    pub avail_width: f32,
    /// This is essentially the height of the parent container
    pub avail_height: f32,
    /// This is the number of children the parent container has
    pub children_at_this_level: u32,
}

macro_rules! create_component_and_table {
    ($ecs:ident, $llty:ty, $name:ident) => {
        let comp: ll::Component<$llty> = $ecs.add_component();
        let $name = $ecs
            .open_session(comp)
            .ok_or(anyhow!("Could not create an ECS session"))?;
    };
}

impl<'a> Dakota<'a> {
    /// Construct a new Dakota instance
    ///
    /// This will initialize the window system platform layer, create a thundr
    /// instance from it, and wrap it in Dakota.
    pub fn new() -> Result<Self> {
        #[cfg(feature = "wayland")]
        let mut plat = platform::WLPlat::new()?;

        #[cfg(feature = "sdl")]
        let mut plat = platform::SDL2Plat::new()?;

        let info = th::CreateInfo::builder()
            .enable_traditional_composition()
            .surface_type(plat.get_th_surf_type()?)
            .build();

        let thundr = th::Thundr::new(&info).context("Failed to initialize Thundr")?;

        let mut layout_ecs = ll::Instance::new();
        create_component_and_table!(layout_ecs, LayoutNode, layout_table);
        create_component_and_table!(layout_ecs, th::Surface, surface_table);
        create_component_and_table!(layout_ecs, DakotaObjectType, types_table);
        create_component_and_table!(layout_ecs, ResMapEntry, resource_map_table);
        create_component_and_table!(layout_ecs, dom::Resource, resource_definitions_table);
        create_component_and_table!(layout_ecs, DakotaId, resources_table);
        create_component_and_table!(layout_ecs, dom::RelativeOffset, offsets_table);
        create_component_and_table!(layout_ecs, dom::RelativeSize, sizes_table);
        create_component_and_table!(layout_ecs, dom::Text, texts_table);
        create_component_and_table!(layout_ecs, dom::Content, content_table);
        create_component_and_table!(layout_ecs, dom::Edges, bounds_table);
        create_component_and_table!(layout_ecs, Vec<DakotaId>, children_table);
        create_component_and_table!(layout_ecs, dom::DakotaDOM, dom_table);

        let mut viewport_ecs = ll::Instance::new();
        create_component_and_table!(viewport_ecs, ViewportNode, viewport_table);

        let dpi = thundr.get_dpi();
        let inst = FontInstance::new(
            "./SourceCodePro-Regular.ttf",
            (dpi.0 as u32, dpi.1 as u32),
            12.0,
        );

        Ok(Self {
            d_plat: plat,
            d_user_fds: None,
            d_thund: thundr,
            d_ecs_inst: layout_ecs,
            d_layout_nodes: layout_table,
            d_layout_node_surfaces: surface_table,
            d_node_types: types_table,
            d_resource_entries: resource_map_table,
            d_resource_definitions: resource_definitions_table,
            d_resources: resources_table,
            d_offsets: offsets_table,
            d_sizes: sizes_table,
            d_texts: texts_table,
            d_contents: content_table,
            d_bounds: bounds_table,
            d_children: children_table,
            d_dom: dom_table,
            d_viewport_ecs_inst: viewport_ecs,
            d_viewport_nodes: viewport_table,
            d_layout_tree_root: None,
            d_root_viewport: None,
            d_window_dims: None,
            d_needs_redraw: false,
            d_needs_refresh: false,
            d_event_sys: EventSystem::new(),
            d_font_inst: inst,
            d_ood_counter: 30,
        })
    }

    /// Create a new toplevel Dakota DOM
    fn create_dakota_dom(&mut self) -> Result<DakotaId> {
        self.create_new_id_common(DakotaObjectType::DakotaDOM)
    }

    /// Create a new Dakota element
    fn create_element(&mut self) -> Result<DakotaId> {
        self.create_new_id_common(DakotaObjectType::Element)
    }

    /// Create a new Dakota resource
    fn create_resource(&mut self) -> Result<DakotaId> {
        self.create_new_id_common(DakotaObjectType::Resource)
    }

    /// Create a new Dakota Id
    ///
    /// The type of the new id must be specified. In Dakota, all objects are
    /// represented by an Id, the type of which is specified during creation.
    /// This type will assign the "role" of this id, and what data can be
    /// attached to it.
    fn create_new_id_common(&mut self, element_type: DakotaObjectType) -> Result<DakotaId> {
        let id = self.d_ecs_inst.add_entity();

        self.set_object_type(&id, element_type);
        return Ok(id);
    }

    /// Reload all of the thundr images from their dakota resources
    ///
    /// Dakota resources may be backed by a Thundr Image. This funciton is in charge
    /// of iterating through all of the dakota resources in use by elements and create
    /// Images for all of them.
    pub fn refresh_resource_map(&mut self, dom_id: &DakotaId) -> Result<()> {
        self.d_thund.clear_all();
        let dom = self
            .d_dom
            .get(dom_id)
            .ok_or(anyhow!("Only DOM objects can be refreshed"))?;

        // Load our resources
        //
        // These get tracked in a resource map so they can be looked up during element creation
        // TODO: don't use this
        for res_id in dom.resource_map.resources.iter() {
            if self.d_resource_entries.get(res_id).is_some() {
                continue;
            }

            let res = self
                .d_resource_definitions
                .get(res_id)
                .ok_or(anyhow!("Could not get Resource"))?;

            let image = match res.image.as_ref() {
                Some(image) => {
                    if image.format != dom::Format::ARGB8888 {
                        return Err(anyhow!("Invalid image format"));
                    }

                    let file_path = image.data.get_fs_path()?;

                    // Create an in-memory representation of the image contents
                    let resolution = image::image_dimensions(std::path::Path::new(file_path))
                        .context(
                        "Format of image could not be guessed correctly. Could not get resolution",
                    )?;
                    let img = image::open(file_path)
                        .context("Could not open image path")?
                        .to_bgra8();
                    let pixels: Vec<u8> = img.into_vec();
                    let mimg = MemImage::new(
                        pixels.as_slice().as_ptr() as *mut u8,
                        4,                     // width of a pixel
                        resolution.0 as usize, // width of texture
                        resolution.1 as usize, // height of texture
                    );

                    // create a thundr image for each resource
                    Some(self.d_thund.create_image_from_bits(&mimg, None).unwrap())
                }
                None => None,
            };

            // Add the new image to our resource map
            self.d_resource_entries.set(
                res_id,
                ResMapEntry {
                    rme_image: image,
                    rme_color: res.color,
                },
            );
        }
        Ok(())
    }

    /// Calculate size and position of centered content.
    ///
    ///
    /// This box has centered content.
    /// We should either recurse the child box or calculate the
    /// size based on the centered resource.
    fn calculate_sizes_content(
        &mut self,
        el: &DakotaId,
        space: &LayoutSpace,
        parent: &mut LayoutNode,
    ) -> Result<()> {
        let child_id = self.d_contents.get(el).unwrap().el.clone();

        // num_children_at_this_level was set earlier to 0 when we
        // created the common child space
        self.calculate_sizes(&child_id, &space)?;
        let mut child_size = self.d_layout_nodes.get_mut(&child_id).unwrap();
        // At this point the size of the is calculated
        // and we can determine the offset. We want to center the
        // box, so that's the center point of the parent minus
        // half the size of the child.
        //
        // The child size should have already been clipped to the available space
        child_size.l_offset.x = utils::partial_max(
            (space.avail_width / 2.0) - (child_size.l_size.width / 2.0),
            0.0,
        );
        child_size.l_offset.y = utils::partial_max(
            (space.avail_height / 2.0) - (child_size.l_size.height / 2.0),
            0.0,
        );

        parent.add_child(child_id.clone());
        Ok(())
    }

    /// Recursively calls calculate_sizes on all children of el
    ///
    /// This does all the work to get information about children for a particular
    /// element. After having the children calculate their sizes, it will assign
    /// them layout positions within el. This will fill from left to right by
    /// default, wrapping below if necessary.
    fn calculate_sizes_children(
        &mut self,
        el: &DakotaId,
        space: &LayoutSpace,
        parent: &mut LayoutNode,
    ) -> Result<()> {
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
            self.calculate_sizes(&child_id, &space)?;
            let mut child_size = self.d_layout_nodes.get_mut(&child_id).unwrap();

            // now the child size has been made, but it still needs to find
            // the proper position inside the parent container. If the child
            // already had an offset specified, it is "out of the loop", and
            // doesn't get used for pretty formatting, it just gets placed
            // wherever.
            if !child_size.l_offset_specified {
                // if this element exceeds the horizontal space, set it on a
                // new line
                if tile_info.t_last_x as f32 + child_size.l_size.width > space.avail_width {
                    tile_info.t_last_x = 0;
                    tile_info.t_last_y = tile_info.t_greatest_y;
                }

                child_size.l_offset = dom::Offset {
                    x: tile_info.t_last_x as f32,
                    y: tile_info.t_last_y as f32,
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

            // Test if the child exceeds the parent space. If so, this is a scrolling
            // region and we should mark it as a viewport boundary. In a separate pass
            // we will go through and create all the viewports.
            if child_size.l_offset.x + child_size.l_size.width > parent.l_size.width
                || child_size.l_offset.y + child_size.l_size.height > parent.l_size.height
            {
                parent.l_is_viewport = true;
            }

            parent.add_child(child_id.clone());
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
    fn calculate_sizes_el(
        &mut self,
        el: &DakotaId,
        node: &mut LayoutNode,
        space: &LayoutSpace,
    ) -> Result<()> {
        node.l_offset_specified = self.get_offset(el).is_some();
        node.l_offset = self
            .get_final_offset(el, &space)
            .context("Failed to calculate offset size of Element")?
            .into();

        node.l_size = self.get_final_size(el, space)?.into();

        Ok(())
    }

    /// Handles creating LayoutNodes for every glyph in a passage
    ///
    /// This is the handler for the text field in the dakota file
    fn calculate_sizes_text(&mut self, el: &DakotaId, node: &mut LayoutNode) -> Result<()> {
        let mut text = self.d_texts.get_mut(el).unwrap();
        let line_space = self.d_font_inst.get_vertical_line_spacing();

        // This is how far we have advanced on a line
        // Go down by one line space before writing the first line. This deals
        // with the problem that ft/hb want to index by the bottom left corner
        // and all my stuff wants to index from the top left corner. Without this
        // text starts being written "above" the element it is assigned to.
        let mut cursor = Cursor {
            c_i: 0,
            c_x: node.l_offset.x,
            c_y: node.l_offset.y + line_space,
            c_min: node.l_offset.x,
            c_max: node.l_offset.x + node.l_size.width,
        };

        log::info!("Drawing text");
        log::info!("{:#?}", cursor);

        // Trim out newlines and tabs. Styling is done with entries in the DOM, not
        // through text formatting in the dakota file.
        for item in text.items.iter_mut() {
            match item {
                dom::TextItem::p(run) | dom::TextItem::b(run) => {
                    // We need to take references to everything at once before the closure
                    // so that the borrow checker can see we aren't trying to reference all
                    // of self
                    let font_inst = &mut self.d_font_inst;
                    let layouts = &mut self.d_layout_nodes;

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
                                    x: (curse.c_x + ch.offset.0).round(),
                                    y: (curse.c_y + ch.offset.1).round(),
                                },
                                dom::Size {
                                    width: size.0,
                                    height: size.1,
                                },
                            );
                            log::info!("Character size is {:?}", size);

                            // Test if the text exceeds the parent space. If so then we need
                            // to mark this node as a viewport node
                            if child_size.l_offset.x + child_size.l_size.width > node.l_size.width
                                || child_size.l_offset.y + child_size.l_size.height
                                    > node.l_size.height
                            {
                                node.l_is_viewport = true;
                            }

                            layouts.take(&ch.node);
                            layouts.set(&ch.node, child_size);
                            // What we have done here is create a "fake" element (fake since
                            // the user didn't specify it) that represents a glyph.
                            node.add_child(ch.node.clone());
                        },
                    );
                }
            }
        }

        Ok(())
    }

    /// Create a new LayoutNode and id pair
    ///
    /// This is a helper for creating a LayoutNode and a matching DakotaId.
    /// We need both because we need a) a node struct holding a bunch of data
    /// and b) we need an ECS ID to perform lookups with.
    #[allow(dead_code)]
    fn create_layout_node(
        &mut self,
        glyph_id: Option<u16>,
        off: dom::Offset<f32>,
        size: dom::Size<f32>,
    ) -> DakotaId {
        let new_id = self.d_ecs_inst.add_entity();
        self.d_layout_nodes
            .set(&new_id, LayoutNode::new(glyph_id, off, size));

        new_id
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
    fn calculate_sizes(&mut self, el: &DakotaId, space: &LayoutSpace) -> Result<()> {
        let mut ret = LayoutNode::new(None, dom::Offset::new(0.0, 0.0), dom::Size::new(0.0, 0.0));

        // ------------------------------------------
        // HANDLE THIS ELEMENT
        // ------------------------------------------
        // Must be done before anything referencing the size of this element
        self.calculate_sizes_el(el, &mut ret, space)
            .context("Layout Tree Calculation: processing element")?;

        // This space is what the children/content will use
        // it is restricted in size to this element (their parent)
        let mut child_space = LayoutSpace {
            avail_width: ret.l_size.width,
            avail_height: ret.l_size.height,
            children_at_this_level: 0,
        };

        // ------------------------------------------
        // HANDLE TEXT
        // ------------------------------------------
        // We do this after handling the size of the current element so that we
        // can know what width we have available to fill in with text.
        if self.get_text(el).is_some() {
            self.calculate_sizes_text(el, &mut ret)?;
        }

        // if the box has children, then recurse through them and calculate our
        // box size based on the fill type.
        if self.get_children(el).is_some() && self.get_children(el).unwrap().len() > 0 {
            // ------------------------------------------
            // CHILDREN
            // ------------------------------------------
            //

            // update our child count
            child_space.children_at_this_level = self.d_children.get(el).unwrap().len() as u32;

            self.calculate_sizes_children(el, &child_space, &mut ret)
                .context("Layout Tree Calculation: processing children of element")?;
        } else if self.get_content(el).is_some() {
            // ------------------------------------------
            // CENTERED CONTENT
            // ------------------------------------------
            self.calculate_sizes_content(el, space, &mut ret)
                .context("Layout Tree Calculation: processing centered content of element")?;
        }

        log::debug!("Final offset of element is {:?}", ret.l_offset);
        log::debug!("Final size of element is {:?}", ret.l_size);
        self.d_layout_nodes.take(el);
        self.d_layout_nodes.set(el, ret);

        return Ok(());
    }

    /// Get the total internal size for this layout node. This is used to calculate
    /// the scrolling region within this node, useful if it is a viewport node.
    fn get_node_internal_size(&self, id: DakotaId) -> (f32, f32) {
        let node = self.d_layout_nodes.get(&id).unwrap();
        let mut ret = (
            node.l_offset.x + node.l_size.width,
            node.l_offset.y + node.l_size.height,
        );

        for child_id in node.l_children.iter() {
            let child = self.d_layout_nodes.get(&child_id).unwrap();

            // If this childs end position is larger, adjust our returning size
            // accordingly
            ret.0 = ret.0.max(child.l_offset.x + child.l_size.width);
            ret.1 = ret.1.max(child.l_offset.y + child.l_size.height);
        }

        return ret;
    }

    fn calculate_viewports(
        &mut self,
        id: DakotaId,
        mut parent_viewport: Option<ViewportId>,
        mut offset: (f32, f32),
    ) -> Option<ViewportId> {
        let node_offset = self.d_layout_nodes.get_mut(&id).unwrap().l_offset;
        offset.0 += node_offset.x;
        offset.1 += node_offset.y;

        {
            if self.d_layout_nodes.get_mut(&id).unwrap().l_is_viewport {
                // Do this first before we mutably borrow node
                let scroll_region = self.get_node_internal_size(id.clone());

                let node = self.d_layout_nodes.get_mut(&id).unwrap();
                let new_id = self.d_viewport_ecs_inst.add_entity();

                // Add this as a child of the parent
                // Do this first to please the borrow checker
                if let Some(parent_id) = parent_viewport.as_ref() {
                    let mut parent = self.d_viewport_nodes.get_mut(&parent_id).unwrap();
                    parent.v_children.push(new_id.clone());
                }

                let mut th_viewport = th::Viewport::new(
                    offset.0 as i32,
                    offset.1 as i32,
                    node.l_size.width as i32,
                    node.l_size.height as i32,
                );
                th_viewport.set_scroll_region(scroll_region.0 as i32, scroll_region.1 as i32);

                let viewport = ViewportNode {
                    v_root_node: Some(id.clone()),
                    v_viewport: th_viewport,
                    v_children: Vec::new(),
                    v_surfaces: th::SurfaceList::new(&mut self.d_thund),
                };
                self.d_viewport_nodes.set(&new_id, viewport);

                parent_viewport = Some(new_id);
            }
        }

        let num_children = self.d_layout_nodes.get(&id).unwrap().l_children.len();
        for i in 0..num_children {
            let child = self.d_layout_nodes.get(&id).unwrap().l_children[i].clone();
            self.calculate_viewports(child.clone(), parent_viewport.clone(), offset);
        }

        return parent_viewport;
    }

    fn clear_viewports(&mut self, id: ViewportId) {
        self.d_viewport_nodes
            .get_mut(&id)
            .unwrap()
            .v_surfaces
            .clear();

        let num_children = self.d_viewport_nodes.get(&id).unwrap().v_children.len();
        for i in 0..num_children {
            let child = self.d_viewport_nodes.get(&id).unwrap().v_children[i].clone();
            self.clear_viewports(child);
        }
    }

    fn clear_thundr_surfaces(&mut self) {
        if let Some(root) = self.d_root_viewport.clone() {
            self.clear_viewports(root);
        }
    }

    /// This takes care of freeing all of our Thundr Images and such.
    /// This isn't handled by th::Image::Drop since we have to call
    /// functions on Thundr to free the image.
    fn clear_thundr(&mut self) {
        self.clear_thundr_surfaces();
        // This destroys all of the images
        self.d_thund.clear_all();
    }

    /// Create the thundr surfaces from the Element layout tree.
    ///
    /// At this point the layout tree should have been constructed, aka
    /// Elements will have their sizes correctly (re)calculated and filled
    /// in by `calculate_sizes`.
    ///
    /// This does not cross viewport boundaries. This function will be called on
    /// the root node for every viewport.
    fn create_thundr_surf_for_el(&mut self, node: &DakotaId) -> Result<th::Surface> {
        let mut surf = {
            let layout = self.d_layout_nodes.get(node).unwrap();

            // If this node is a viewport then ignore its offset since its surface
            // is going to be added to a different surfacelist
            let offset = match layout.l_is_viewport {
                true => (0.0, 0.0),
                false => (layout.l_offset.x, layout.l_offset.y),
            };

            // first create a surface for this element, or get an existing one
            // This starts as an empty unbound surface but may be assigned content below
            let mut surf = if self.d_layout_node_surfaces.get_mut(node).is_some() {
                // Do this here to avoid hogging the borrow with the above line
                let mut surf = self.d_layout_node_surfaces.get_mut(node).unwrap();
                surf.reset_surface(
                    offset.0,
                    offset.1,
                    layout.l_size.width,
                    layout.l_size.height,
                );
                surf.clone()
            } else {
                let surf = self.d_thund.create_surface(
                    offset.0,
                    offset.1,
                    layout.l_size.width,
                    layout.l_size.height,
                );
                // Set and get this to match the RefMut in the first if branch
                self.d_layout_node_surfaces.set(node, surf.clone());
                surf
            };

            // Handle binding images
            // We need to get the resource's content from our resource map, get
            // the thundr image for it, and bind it to our new surface.
            if let Some(resource_id) = self.d_resources.get(node) {
                if let Some(rme) = self.d_resource_entries.get(&resource_id) {
                    // Assert that only one content type is set
                    let mut content_num = 0;
                    if rme.rme_image.is_some() {
                        content_num += 1;
                    }
                    if rme.rme_color.is_some() {
                        content_num += 1;
                    }
                    assert!(content_num == 1);

                    if let Some(image) = rme.rme_image.as_ref() {
                        self.d_thund.bind_image(&mut surf, image.clone());
                    }
                    if let Some(color) = rme.rme_color.as_ref() {
                        surf.set_color((color.r, color.g, color.b, color.a));
                    }
                }
            } else if let Some(glyph_id) = layout.l_glyph_id {
                // If this path is hit, then this layout node is really a glyph in a
                // larger block of text. It has been created as a child, and isn't
                // a real element. We ask the font code to give us a surface for
                // it that we can display.
                self.d_font_inst.get_thundr_surf_for_glyph(
                    &mut self.d_thund,
                    &mut surf,
                    glyph_id,
                    layout.l_offset,
                );

                return Ok(surf);
            }

            surf
        };

        // now iterate through all of it's children, and recursively do the same
        // This is written kind of weird to work around some annoying borrow checker
        // bits. By not referencing self in the for loop we can avoid double
        // mut reffing self and hitting borrow checker issues
        let num_children = self.d_layout_nodes.get(&node).unwrap().l_children.len();
        for i in 0..num_children {
            let child_id = {
                let layout = self.d_layout_nodes.get(&node).unwrap();
                layout.l_children[i].clone()
            };
            // add the new child surface as a subsurface
            // don't do this if this is a viewport boundary
            if !self.d_layout_nodes.get(&child_id).unwrap().l_is_viewport {
                let child_surf = self.create_thundr_surf_for_el(&child_id)?;
                surf.add_subsurface(child_surf);
            }
        }

        return Ok(surf);
    }

    /// Helper method to print out our layout tree
    #[allow(dead_code)]
    #[cfg(debug_assertions)]
    fn print_node(&self, _id: &DakotaId, node: &LayoutNode, indent_level: usize) {
        let spaces = std::iter::repeat("  ")
            .take(indent_level)
            .collect::<String>();

        log::verbose!("{}Layout node:", spaces);
        log::verbose!(
            "{}    offset={:?}, size={:?}",
            spaces,
            node.l_offset,
            node.l_size
        );

        log::verbose!(
            "{}    glyph_id={:?}, num_children={}, is_viewport={}",
            spaces,
            node.l_glyph_id,
            node.l_children.len(),
            node.l_is_viewport,
        );

        for child_id in node.l_children.iter() {
            let child = &self.d_layout_nodes.get(child_id).unwrap();
            self.print_node(child_id, child, indent_level + 1);
        }
    }

    /// This pass recursively generates the surfacelists for each
    /// Viewport in the scene.
    fn calculate_thundr_surfaces(&mut self, id: ViewportId) -> Result<()> {
        let root_node_raw = self.d_viewport_nodes.get(&id).unwrap().v_root_node.clone();
        if let Some(root_node_id) = root_node_raw {
            // Create our thundr surface and add it to the list
            let root_surf = self
                .create_thundr_surf_for_el(&root_node_id)
                .context("Could not construct Thundr surface tree")?;

            let viewport = &mut self.d_viewport_nodes.get_mut(&id).unwrap();
            viewport.v_surfaces.clear();
            viewport.v_surfaces.push(root_surf.clone());
        }

        let num_children = self.d_viewport_nodes.get(&id).unwrap().v_children.len();
        for i in 0..num_children {
            let child_viewport = self.d_viewport_nodes.get(&id).unwrap().v_children[i].clone();
            self.calculate_thundr_surfaces(child_viewport)?;
        }

        Ok(())
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
    /// type.
    pub fn add_child_to_element(&mut self, parent: &DakotaId, child: DakotaId) {
        // Assert this id has the Element type
        self.assert_id_has_type(parent, DakotaObjectType::Element);
        self.assert_id_has_type(&child, DakotaObjectType::Element);

        // Add old_id as a child element
        if self.d_children.get_mut(parent).is_none() {
            self.d_children.set(parent, Vec::new());
        }

        self.d_children.get_mut(parent).unwrap().push(child);
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
                self.d_window_dims = Some((dom.window.width, dom.window.height));

                // we need to update the window dimensions if possible,
                // so call into our platform do handle it
                self.d_plat
                    .set_output_params(&dom.window, self.d_window_dims.unwrap())?;
            }
            dom.root_element.clone()
        };

        // reset our thundr surface list. If the set of resources has
        // changed, then we should have called clear_thundr to do so by now.
        self.clear_thundr_surfaces();
        self.d_root_viewport = None;
        self.d_layout_tree_root = None;

        // construct layout tree with sizes of all boxes
        self.calculate_sizes(
            &root_node_id,
            &LayoutSpace {
                avail_width: self.d_window_dims.unwrap().0 as f32, // available width
                avail_height: self.d_window_dims.unwrap().1 as f32, // available height
                children_at_this_level: 1,                         // Only one child, the root node
            },
        )?;
        // Manually mark the root node as a viewport node. It always is, and it will
        // always have the root viewport.
        self.d_layout_nodes
            .get_mut(&root_node_id)
            .unwrap()
            .l_is_viewport = true;

        // Perform our viewport pass
        //
        // This will go through the layout tree and create a tree of ViewportNodes
        // to represent the different scrolling regions within the scene.
        self.d_root_viewport = self.calculate_viewports(root_node_id.clone(), None, (0.0, 0.0));

        //#[cfg(debug_assertions)]
        //{
        //    if let Some(root_id) = self.d_layout_tree_root.as_ref() {
        //        self.print_node(&self.d_layout_nodes.get(&root_id).unwrap(), 0);
        //    }
        //}

        // Perform the Thundr pass
        //
        // This generates thundr resources for all viewports and nodes in the
        // layout tree. This is the last step needed before drawing.
        // We can expect the root viewport to exist since we just did it above
        self.calculate_thundr_surfaces(self.d_root_viewport.clone().unwrap())?;

        self.d_layout_tree_root = Some(root_node_id);
        self.d_needs_refresh = false;

        Ok(())
    }

    /// Completely flush the thundr surfaces/images and recreate the scene
    pub fn refresh_full(&mut self, dom: &DakotaId) -> Result<()> {
        self.d_needs_redraw = true;
        self.clear_thundr();
        self.refresh_resource_map(dom)
            .context("Refreshing resource map")?;
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

    fn viewport_at_pos_recursive(&self, id: ViewportId, x: i32, y: i32) -> Option<ViewportId> {
        let node = self.d_viewport_nodes.get(&id).unwrap();
        let viewport = &node.v_viewport;

        // Since the viewport tree is back to front, process the children first. If one
        // of them is a match, it is the top-most viewport and we should return it. Otherwise
        // we can test if this viewport matches
        for child in node.v_children.iter() {
            if let Some(ret) = self.viewport_at_pos_recursive(child.clone(), x, y) {
                return Some(ret);
            }
        }

        let x_range = viewport.offset.0..(viewport.offset.0 + viewport.size.0);
        let y_range = viewport.offset.1..(viewport.offset.1 + viewport.size.1);

        if x_range.contains(&x) && y_range.contains(&y) {
            return Some(id);
        }

        None
    }

    /// Walks the viewport tree and returns the ECS id of the
    /// viewport at this location. Note there will always be a viewport
    /// because the entire window surface is at the very least, the root viewport
    fn viewport_at_pos(&self, x: i32, y: i32) -> ViewportId {
        assert!(self.d_root_viewport.is_some());
        let root_node = self.d_root_viewport.clone().unwrap();

        match self.viewport_at_pos_recursive(root_node.clone(), x, y) {
            Some(v) => v,
            None => root_node,
        }
    }

    /// Handle dakota-only events coming from the event system
    ///
    /// Most notably this handles scrolling
    fn handle_private_events(&mut self) -> Result<()> {
        for ev in self.d_event_sys.es_dakota_event_queue.iter() {
            match ev {
                Event::InputScroll {
                    mouse_x,
                    mouse_y,
                    x,
                    y,
                } => {
                    // Find viewport at this location
                    let viewport = self.viewport_at_pos(*mouse_x, *mouse_y);

                    // set its scrolling offset to be used for the next draw
                    let mut node = self.d_viewport_nodes.get_mut(&viewport).unwrap();

                    node.v_viewport.set_scroll_amount(*x as i32, *y as i32);
                    self.d_needs_redraw = true;
                }
                // Ignore all other events for now
                _ => {}
            }
        }

        self.d_event_sys.es_dakota_event_queue.clear();
        Ok(())
    }

    fn flush_viewports(&mut self, viewport: ViewportId) -> th::Result<()> {
        {
            let mut node = self.d_viewport_nodes.get_mut(&viewport).unwrap();
            self.d_thund.flush_surface_data(&mut node.v_surfaces)?;
        }

        let num_children = self
            .d_viewport_nodes
            .get(&viewport)
            .unwrap()
            .v_children
            .len();
        for i in 0..num_children {
            let child_id = self.d_viewport_nodes.get(&viewport).unwrap().v_children[i].clone();
            self.flush_viewports(child_id)?;
        }

        Ok(())
    }

    fn draw_viewports(&mut self, viewport: ViewportId) -> th::Result<()> {
        {
            let node = self.d_viewport_nodes.get_mut(&viewport).unwrap();
            self.d_thund
                .draw_surfaces(&node.v_surfaces, &node.v_viewport)?;
        }

        let num_children = self
            .d_viewport_nodes
            .get(&viewport)
            .unwrap()
            .v_children
            .len();
        for i in 0..num_children {
            let child_id = self.d_viewport_nodes.get(&viewport).unwrap().v_children[i].clone();
            self.draw_viewports(child_id)?;
        }

        Ok(())
    }

    fn draw_surfacelists(&mut self) -> th::Result<()> {
        let root_viewport = self
            .d_root_viewport
            .clone()
            .expect("Dakota bug: root viewport not valid");

        self.flush_viewports(root_viewport.clone())?;

        self.d_thund.begin_recording()?;
        self.draw_viewports(root_viewport)?;
        self.d_thund.end_recording()?;

        Ok(())
    }

    /// Add a file descriptor to watch
    ///
    /// This will add a new file descriptor to the watch set inside dakota,
    /// meaning dakota will return control to the user when this fd is readable.
    /// This is done through the `UserFdReadable` event.
    pub fn add_watch_fd(&mut self, fd: RawFd) {
        if self.d_user_fds.is_none() {
            self.d_user_fds = Some(FdWatch::new());
        }

        let watch = self.d_user_fds.as_mut().unwrap();
        watch.add_fd(fd);
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
    pub fn dispatch(&mut self, dom: &DakotaId, mut timeout: Option<u32>) -> Result<()> {
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
    pub fn dispatch_platform(&mut self, dom: &DakotaId, timeout: Option<u32>) -> Result<()> {
        // First run our window system code. This will check if wayland/X11
        // notified us of a resize, closure, or need to redraw
        let plat_res = self.d_plat.run(
            &mut self.d_event_sys,
            self.d_dom
                .get(dom)
                .ok_or(anyhow!("Id passed to Dispatch must be a DOM object"))?
                .deref(),
            timeout,
            self.d_user_fds.as_mut(),
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
                self.d_thund.handle_ood();
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

        if self.d_needs_refresh {
            let mut layout_stop = StopWatch::new();
            layout_stop.start();
            self.refresh_elements(dom)?;
            layout_stop.end();
            log::error!(
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
            log::error!(
                "Dakota spent {} ms drawing this frame",
                stop.get_duration().as_millis()
            );
        }

        return Ok(());
    }
}
