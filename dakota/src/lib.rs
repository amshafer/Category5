extern crate image;
extern crate serde;
extern crate thundr as th;
pub use th::ThundrError as DakotaError;

extern crate bitflags;

extern crate lazy_static;
extern crate utils;
use utils::log;
pub use utils::{anyhow, ecs::*, region::Rect, Context, Error, MemImage, Result};

pub mod dom;
pub mod input;
use dom::DakotaDOM;
mod platform;
use platform::Platform;
pub mod xml;

pub mod event;
use event::{Event, EventSystem};

mod font;
use font::*;

use std::collections::HashMap;
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

pub type LayoutId = ECSId;

pub struct Dakota<'a> {
    // GROSS: we need thund to be before plat so that it gets dropped first
    // It might reference the window inside plat, and will segfault if
    // dropped after it.
    d_thund: th::Thundr,
    #[cfg(feature = "wayland")]
    d_plat: platform::WlPlat,
    #[cfg(feature = "sdl")]
    d_plat: platform::SDL2Plat,
    d_resmap: HashMap<String, ResMapEntry>,
    d_surfaces: th::SurfaceList,
    /// This is one ECS that is composed of multiple tables
    d_layout_ecs_inst: ECSInstance,
    /// This is one such ECS table, that holds all the layout nodes
    d_layout_nodes: ECSTable<LayoutNode>,
    /// This is the root node in the scene tree
    d_layout_tree_root: Option<LayoutId>,
    d_window_dims: Option<(u32, u32)>,
    d_needs_redraw: bool,
    d_event_sys: EventSystem,
    d_font_inst: FontInstance<'a>,
}

/// The elements of the layout tree.
/// This will be constructed from the Elements in the DOM
#[derive(Debug)]
struct LayoutNode {
    /// Has this element been assigned a resource?
    l_resource: Option<String>,
    /// Is this element a glyph subsurface. If so it is one character
    /// in a block of text. This is really an index into the font.
    l_glyph_id: Option<u16>,
    /// True if the dakota file specified an offset for this el
    l_offset_specified: bool,
    l_offset: dom::Offset<f32>,
    l_size: dom::Size<f32>,
    /// Ids of the children that this layout node has
    l_children: Vec<LayoutId>,
}

impl Default for LayoutNode {
    fn default() -> Self {
        Self {
            l_resource: None,
            l_glyph_id: None,
            l_offset_specified: false,
            l_offset: dom::Offset::new(0.0, 0.0),
            l_size: dom::Size::new(0.0, 0.0),
            l_children: Vec::with_capacity(0),
        }
    }
}

impl LayoutNode {
    fn new(
        res: Option<String>,
        glyph_id: Option<u16>,
        off: dom::Offset<f32>,
        size: dom::Size<f32>,
    ) -> Self {
        Self {
            l_resource: res,
            l_glyph_id: glyph_id,
            l_offset_specified: false,
            l_offset: off,
            l_size: size,
            l_children: Vec::with_capacity(0),
        }
    }

    fn add_child(&mut self, other: ECSId) {
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
            let other = &dakota.d_layout_nodes[child_id];

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

        let layout_ecs = ECSInstance::new();
        let layout_table = ECSTable::new(layout_ecs.clone());

        let inst = FontInstance::new("./Ubuntu-Regular.ttf", thundr.get_dpi() as u32, 11.0);

        Ok(Self {
            d_plat: plat,
            d_thund: thundr,
            d_surfaces: th::SurfaceList::new(),
            d_resmap: HashMap::new(),
            d_layout_ecs_inst: layout_ecs,
            d_layout_nodes: layout_table,
            d_layout_tree_root: None,
            d_window_dims: None,
            d_needs_redraw: false,
            d_event_sys: EventSystem::new(),
            d_font_inst: inst,
        })
    }

    pub fn refresh_resource_map(&mut self, dom: &DakotaDOM) -> Result<()> {
        self.d_thund.clear_all();

        // Load our resources
        //
        // These get tracked in a resource map so they can be looked up during element creation
        for res in dom.resource_map.resources.iter() {
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
                        .context(format!("Could not open image: {:?}", file_path))?
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
            self.d_resmap.insert(
                res.name.clone(),
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
        content: &dom::Content,
        space: &LayoutSpace,
        parent: &mut LayoutNode,
    ) -> Result<()> {
        if let Some(child) = content.el.as_ref() {
            // num_children_at_this_level was set earlier to 0 when we
            // created the common child space
            let child_id = self.calculate_sizes(&mut child.borrow_mut(), &space)?;
            let mut child_size = &mut self.d_layout_nodes[&child_id];
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
        }
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
        el: &mut dom::Element,
        space: &LayoutSpace,
        parent: &mut LayoutNode,
    ) -> Result<()> {
        // TODO: do vertical wrapping too
        let mut tile_info = TileInfo {
            t_last_x: 0,
            t_last_y: 0,
            t_greatest_y: 0,
        };

        for child in el.children.iter() {
            let child_id = self.calculate_sizes(&mut child.borrow_mut(), &space)?;
            let mut child_size = &mut self.d_layout_nodes[&child_id];

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

            parent.add_child(child_id.clone());
        }

        Ok(())
    }

    /// Calculate the sizes and handle the current element
    ///
    /// Now that we have calculated all the children, we can handle
    /// this element.
    /// 1. If it has a size assigned, that is the final size, all children
    /// will be clipped or scrolled inside that window.
    /// 2. If no size is assigned, and we are limited in the amount of space
    /// we have, then the size is available_space
    /// 3. No size and no bounds means we are inside of a scrolling arena, and
    /// we should grow this box to hold all of its children.
    fn calculate_sizes_el(
        &mut self,
        el: &mut dom::Element,
        node: &mut LayoutNode,
        space: &LayoutSpace,
    ) -> Result<()> {
        if let Some(off) = el.get_final_offset(&space).context(format!(
            "Failed to calculate offset size of Element {:#?}",
            el
        ))? {
            node.l_offset_specified = true;
            node.l_offset = off.into();
        }

        if let Some(size) = el.get_final_size(space)? {
            node.l_size = size.into();
        } else {
            // first grow this box to fit its children.
            // TODO: this element's size should be set and children
            // will have to be contained within it.
            //node.resize_to_children(self)?;

            if node.l_size == dom::Size::new(0.0, 0.0) {
                // if the size is still empty, there were no children. This should just be
                // sized to the available space divided by the number of
                // children.
                // Clamp to 1 to avoid dividing by zero
                let num_children = std::cmp::max(1, space.children_at_this_level);
                // TODO: add directional tiling of elements
                // for now just do vertical subdivision and fill horizontal
                node.l_size =
                    dom::Size::new(space.avail_width, space.avail_height / num_children as f32);
            }

            // Then possibly clip the box by any available dimensions.
            // Add our offsets while calculating this to account for space
            // used by moving the box.

            // TODO: don't clamp, add scrolling support
            node.l_size.width = node
                .l_size
                .width
                .clamp(0.0, space.avail_width - node.l_offset.x);
            node.l_size.height =
                utils::partial_min(space.avail_height + node.l_offset.y, node.l_size.height);
        }

        Ok(())
    }

    /// Handles creating LayoutNodes for every glyph in a passage
    ///
    /// This is the handler for the text field in the dakota file
    fn calculate_sizes_text(&mut self, text: &mut dom::Text, node: &mut LayoutNode) -> Result<()> {
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

        println!("Drawing text");
        println!("{:#?}", cursor);

        // Trim out newlines and tabs. Styling is done with entries in the DOM, not
        // through text formatting in the dakota file.
        for item in text.items.iter_mut() {
            match item {
                dom::TextItem::p(s) | dom::TextItem::b(s) => {
                    // TODO: we can get the available height from above, pass it to a font instance
                    // and create layout nodes for all character surfaces.
                    let trim = regex_trim_excess_space(s);

                    // We need to take references to everything at once before the closure
                    // so that the borrow checker can see we aren't trying to reference all
                    // of self
                    let font_inst = &mut self.d_font_inst;
                    let ecs_inst = &mut self.d_layout_ecs_inst;
                    let layouts = &mut self.d_layout_nodes;

                    // Record text locations
                    // We will create a whole bunch of sub-nodes which will be assigned
                    // glyph ids. These ids will later be used to get surfaces for.
                    font_inst.layout_text(
                        &mut self.d_thund,
                        &mut cursor,
                        &trim,
                        &mut |inst: &mut FontInstance, thund, glyph_id, offset| {
                            let size = inst.get_glyph_thundr_size(thund, glyph_id);
                            let new_id = ecs_inst.mint_new_id();
                            layouts[&new_id] = LayoutNode::new(
                                None,
                                Some(glyph_id),
                                dom::Offset {
                                    x: offset.0,
                                    y: offset.1,
                                },
                                dom::Size {
                                    width: size.0,
                                    height: size.1,
                                },
                            );
                            node.add_child(new_id);
                        },
                    );

                    *s = trim;
                }
            }
        }

        println!("Dumping children of node");
        for l in node.l_children.iter() {
            println!("Child");
            println!("{:#?}", self.d_layout_nodes[l].l_offset);
            println!("{:#?}", self.d_layout_nodes[l].l_size);
        }
        println!("{:#?}", text);
        Ok(())
    }

    /// Create a new LayoutNode and id pair
    ///
    /// This is a helper for creating a LayoutNode and a matching LayoutId.
    /// We need both because we need a) a node struct holding a bunch of data
    /// and b) we need an ECS ID to perform lookups with.
    #[allow(dead_code)]
    fn create_layout_node(
        &mut self,
        res: Option<String>,
        glyph_id: Option<u16>,
        off: dom::Offset<f32>,
        size: dom::Size<f32>,
    ) -> LayoutId {
        let new_id = self.d_layout_ecs_inst.mint_new_id();
        self.d_layout_nodes[&new_id] = LayoutNode::new(res, glyph_id, off, size);

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
    fn calculate_sizes(&mut self, el: &mut dom::Element, space: &LayoutSpace) -> Result<LayoutId> {
        let new_id = self.d_layout_ecs_inst.mint_new_id();
        el.layout_id = Some(new_id.clone());
        let mut ret = LayoutNode::new(
            el.resource.clone(),
            None,
            dom::Offset::new(0.0, 0.0),
            dom::Size::new(0.0, 0.0),
        );

        // This space is what the children/content will use
        // it is restricted in size to this element (their parent)
        let mut child_space = LayoutSpace {
            avail_width: space.avail_width,
            avail_height: space.avail_height,
            children_at_this_level: 0,
        };

        // check if this element has its size set, shrink the available space
        // to match.
        if let Some(size) = el.size.as_ref() {
            child_space.avail_width = size.width as f32;
            child_space.avail_height = size.height as f32;
        }

        // if the box has children, then recurse through them and calculate our
        // box size based on the fill type.
        if el.children.len() > 0 {
            // ------------------------------------------
            // CHILDREN
            // ------------------------------------------
            //

            // update our child count
            child_space.children_at_this_level = el.children.len() as u32;

            self.calculate_sizes_children(el, &child_space, &mut ret)
                .context(format!(
                    "Layout Tree Calculation: processing children of element {:#?}",
                    el
                ))?;
        } else if let Some(content) = el.content.as_ref() {
            // ------------------------------------------
            // CENTERED CONTENT
            // ------------------------------------------
            self.calculate_sizes_content(content, space, &mut ret)
                .context(format!(
                    "Layout Tree Calculation: processing centered content {:#?} of element {:#?}",
                    content, el
                ))?;
        }

        // ------------------------------------------
        // HANDLE THIS ELEMENT
        // ------------------------------------------
        // Must be done before anything referencing the size of this element
        self.calculate_sizes_el(el, &mut ret, space)
            .context(format!(
                "Layout Tree Calculation: processing element {:#?}",
                el
            ))?;

        // ------------------------------------------
        // HANDLE TEXT
        // ------------------------------------------
        // We do this after handling the size of the current element so that we
        // can know what width we have available to fill in with text.
        if let Some(text) = el.text.as_mut() {
            self.calculate_sizes_text(text, &mut ret)?;
        }

        log::debug!("Final size of element is {:?}", ret);
        self.d_layout_nodes[&new_id] = ret;

        return Ok(new_id);
    }

    /// This takes care of freeing all of our Thundr Images and such.
    /// This isn't handled by th::Image::Drop since we have to call
    /// functions on Thundr to free the image.
    fn clear_thundr(&mut self) {
        // This drops our surfaces
        self.d_surfaces.clear();
        // This destroys all of the images
        self.d_thund.clear_all();
    }

    /// Create the thundr surfaces from the Element layout tree.
    ///
    /// At this point the layout tree should have been constructed, aka
    /// Elements will have their sizes correctly (re)calculated and filled
    /// in by `calculate_sizes`.
    fn create_thundr_surf_for_el(
        &mut self,
        node: LayoutId,
        poffset: dom::Offset<f32>,
    ) -> Result<Option<th::Surface>> {
        let layout = &self.d_layout_nodes[&node];
        // TODO: optimize
        // this is gross but we have to do it for the borrow checker to be happy
        // Otherwise calling the &mut self functions throws errors
        let layout_children = layout.l_children.clone();

        let offset = dom::Offset {
            x: layout.l_offset.x + poffset.x,
            y: layout.l_offset.y + poffset.y,
        };
        let size = layout.l_size;

        if let Some(resname) = layout.l_resource.as_ref() {
            // first create a surface for this element
            let mut surf = self.d_thund.create_surface(
                offset.x as f32,
                offset.y as f32,
                size.width as f32,
                size.height as f32,
            );

            // We need to get the resource's content from our resource map, get
            // the thundr image for it, and bind it to our new surface.
            let rme = match self.d_resmap.get(resname) {
                Some(rme) => rme,
                None => {
                    return Err(anyhow!(
                        "This Element references resource {:?}, which does not exist",
                        resname
                    ))
                }
            };

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
            self.d_surfaces.push(surf.clone());

            // now iterate through all of it's children, and recursively do the same
            for child_id in layout_children.iter() {
                // add the new child surface as a subsurface
                let child_surf = self.create_thundr_surf_for_el(child_id.clone(), offset)?;
                if let Some(csurf) = child_surf {
                    surf.add_subsurface(csurf);
                }
            }

            return Ok(Some(surf));
        } else if let Some(glyph_id) = layout.l_glyph_id {
            // If this path is hit, then this layout node is really a glyph in a
            // larger block of text. It has been created as a child, and isn't
            // a real element. We ask the font code to give us a surface for
            // it that we can display.
            let surf =
                self.d_font_inst
                    .get_thundr_surf_for_glyph(&mut self.d_thund, glyph_id, offset);

            return Ok(Some(surf));
        }

        // if we are here, then the current element does not have content.
        // Instead what we do is recursively call this function on the
        // children, and append them to the surfacelist.
        for child_id in layout_children.iter() {
            // add the new child surface as a subsurface
            let child = self.create_thundr_surf_for_el(child_id.clone(), offset)?;
            if let Some(child_surf) = child {
                self.d_surfaces.push(child_surf);
            }
        }
        return Ok(None);
    }

    /// This refreshes the entire scene, and regenerates
    /// the Thundr surface list.
    pub fn refresh_elements(&mut self, dom: &DakotaDOM) -> Result<()> {
        // check if the window size is set. If it is not, this is the
        // first iteration and we need to populate the dimensions
        // from the DOM
        if self.d_window_dims.is_none() {
            self.d_window_dims = Some((dom.window.width, dom.window.height));
        }

        // we need to update the window dimensions if possible,
        // so call into our platform do handle it
        self.d_plat
            .set_output_params(&dom.window, self.d_window_dims.unwrap())?;

        // construct layout tree with sizes of all boxes
        // create our thundr surfaces while we are at it.
        let num_children = dom.layout.root_element.borrow().children.len() as u32;
        self.d_layout_tree_root = Some(self.calculate_sizes(
            &mut dom.layout.root_element.borrow_mut(),
            &LayoutSpace {
                avail_width: self.d_window_dims.unwrap().0 as f32, // available width
                avail_height: self.d_window_dims.unwrap().1 as f32, // available height
                children_at_this_level: num_children,
            },
        )?);

        // reset our thundr surface list. If the set of resources has
        // changed, then we should have called clear_thundr to do so by now.
        self.d_surfaces.clear();

        // Create our thundr surface and add it to the list
        // one list with subsurfaces?
        let root_node_id = self.d_layout_tree_root.as_ref().unwrap().clone();
        self.create_thundr_surf_for_el(root_node_id, dom::Offset { x: 0.0, y: 0.0 })
            .context("Could not construct Thundr surface tree")?;

        Ok(())
    }

    /// Completely flush the thundr surfaces/images and recreate the scene
    pub fn refresh_full(&mut self, dom: &DakotaDOM) -> Result<()> {
        self.d_needs_redraw = true;
        self.clear_thundr();
        self.refresh_resource_map(dom)?;
        self.refresh_elements(dom)
    }

    /// Handle vulkan swapchain out of date. This is probably because the
    /// window's size has changed. This will requery the window size and
    /// refresh the layout tree.
    fn handle_ood(&mut self, dom: &DakotaDOM) -> Result<()> {
        let new_res = self.d_thund.get_resolution();
        self.d_event_sys.add_event_window_resized(
            dom,
            dom::Size {
                width: new_res.0,
                height: new_res.1,
            },
        );

        self.d_needs_redraw = true;
        self.d_window_dims = Some(new_res);
        self.refresh_elements(dom)
    }

    /// Get the slice of currently unhandled events
    ///
    /// The app should do this in its main loop after dispatching.
    /// These will be cleared during each dispatch.
    pub fn get_events<'b>(&'b self) -> &'b [Event] {
        self.d_event_sys.get_events()
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
    pub fn dispatch(&mut self, dom: &DakotaDOM, timeout: Option<u32>) -> Result<()> {
        // first clear the event queue, the app already had a chance to
        // handle them
        self.d_event_sys.clear_event_queue();

        // First run our window system code. This will check if wayland/X11
        // notified us of a resize, closure, or need to redraw
        match self.d_plat.run(&mut self.d_event_sys, dom, timeout) {
            Ok(()) => {}
            Err(th::ThundrError::OUT_OF_DATE) => {
                // This is a weird one
                // So the above OUT_OF_DATEs are returned from thundr, where we
                // can expect it will handle OOD itself. But here we have
                // OUT_OF_DATE returned from our SDL2 backend, so we need
                // to tell Thundr to do OOD itself
                self.d_thund.handle_ood();
                self.handle_ood(dom)?;
                return Ok(());
            }
            Err(e) => return Err(Error::from(e).context("Thundr: presentation failed")),
        };

        // if needs redraw, then tell thundr to draw and present a frame
        // At every step of the way we check if the drawable has been resized
        // and will return that to the dakota user so they have a chance to resize
        // anything they want
        if self.d_needs_redraw {
            match self.d_thund.draw_frame(&mut self.d_surfaces) {
                Ok(()) => {}
                Err(th::ThundrError::OUT_OF_DATE) => {
                    self.handle_ood(dom)?;
                    return Ok(());
                }
                Err(e) => return Err(Error::from(e).context("Thundr: drawing failed with error")),
            };
            match self.d_thund.present() {
                Ok(()) => {}
                Err(th::ThundrError::OUT_OF_DATE) => {
                    self.handle_ood(dom)?;
                    return Ok(());
                }
                Err(e) => return Err(Error::from(e).context("Thundr: presentation failed")),
            };
            self.d_needs_redraw = false;

            // Notify the app that we just drew a frame and it should prepare the next one
            self.d_event_sys.add_event_window_redraw_complete(dom);
        }

        return Ok(());
    }
}
