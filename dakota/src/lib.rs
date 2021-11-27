extern crate image;
extern crate serde;
extern crate thundr as th;
pub use th::ThundrError as DakotaError;

extern crate utils;
use utils::log;
pub use utils::{anyhow, region::Rect, Context, Error, MemImage, Result};

pub mod dom;
use dom::DakotaDOM;
mod platform;
use platform::Platform;
pub mod xml;

use std::collections::HashMap;

struct ResMapEntry {
    rme_image: Option<th::Image>,
    rme_color: Option<dom::Color>,
}

pub struct Dakota {
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
    d_dom: Option<DakotaDOM>,
    d_layout_tree: Option<LayoutNode>,
    d_window_dims: Option<(u32, u32)>,
}

/// The elements of the layout tree.
/// This will be constructed from the Elements in the DOM
#[derive(Debug)]
struct LayoutNode {
    l_resource: Option<String>,
    /// True if the dakota file specified an offset for this el
    l_offset_specified: bool,
    l_offset: dom::Offset,
    l_size: dom::Size,
    l_children: Vec<LayoutNode>,
}

impl LayoutNode {
    fn new(res: Option<String>, off: dom::Offset, size: dom::Size) -> Self {
        Self {
            l_resource: res,
            l_offset_specified: false,
            l_offset: off,
            l_size: size,
            l_children: Vec::with_capacity(0),
        }
    }

    fn add_child(&mut self, other: LayoutNode) {
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
    pub fn resize_to_children(&mut self) -> Result<()> {
        for other in self.l_children.iter() {
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
struct LayoutSpace {
    /// This is essentially the width of the parent container
    avail_width: u32,
    /// This is essentially the height of the parent container
    avail_height: u32,
    /// This is the number of children the parent container has
    children_at_this_level: u32,
}

impl Dakota {
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

        Ok(Self {
            d_plat: plat,
            d_thund: thundr,
            d_surfaces: th::SurfaceList::new(),
            d_resmap: HashMap::new(),
            d_dom: None,
            d_layout_tree: None,
            d_window_dims: None,
        })
    }

    pub fn refresh_resource_map(&mut self) -> Result<()> {
        let dom = match &mut self.d_dom {
            Some(dom) => dom,
            None => {
                return Err(anyhow!(
                    "A scene is not loaded in Dakota. Please load one from xml",
                ))
            }
        };
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
    fn calculate_sizes(&mut self, el: &dom::Element, mut space: LayoutSpace) -> Result<LayoutNode> {
        let mut ret = LayoutNode::new(
            el.resource.clone(),
            dom::Offset::new(0, 0),
            dom::Size::new(0, 0),
        );

        // check if this element has its size set, shrink the available space
        // to match.
        if let Some(size) = el.size.as_ref() {
            space.avail_width = size.width;
            space.avail_height = size.height;
        }

        // if the box has children, then recurse through them and calculate our
        // box size based on the fill type.
        if el.children.len() > 0 {
            // ------------------------------------------
            // CHILDREN
            // ------------------------------------------
            //
            let child_space = LayoutSpace {
                avail_width: space.avail_width,
                avail_height: space.avail_height,
                children_at_this_level: el.children.len() as u32,
            };

            // TODO: do vertical wrapping too
            let mut tile_info = TileInfo {
                t_last_x: 0,
                t_last_y: 0,
                t_greatest_y: 0,
            };

            for child in el.children.iter() {
                let mut child_size = self.calculate_sizes(child, child_space.clone())?;

                // now the child size has been made, but it still needs to find
                // the proper position inside the parent container. If the child
                // already had an offset specified, it is "out of the loop", and
                // doesn't get used for pretty formatting, it just gets placed
                // wherever.
                if !child_size.l_offset_specified {
                    // if this element exceeds the horizontal space, set it on a
                    // new line
                    if tile_info.t_last_x + child_size.l_size.width > space.avail_width {
                        tile_info.t_last_x = 0;
                        tile_info.t_last_y = tile_info.t_greatest_y;
                    }

                    child_size.l_offset = dom::Offset {
                        x: tile_info.t_last_x,
                        y: tile_info.t_last_y,
                    };

                    // now we need to update the space that we have seen children
                    // occupy, so we know where to place the next children in the
                    // tiling formation.
                    tile_info.t_last_x += child_size.l_size.width;
                    tile_info.t_greatest_y = std::cmp::max(
                        tile_info.t_greatest_y,
                        tile_info.t_last_y + child_size.l_size.height,
                    );
                }

                ret.add_child(child_size);
            }
        } else if let Some(content) = el.content.as_ref() {
            // ------------------------------------------
            // CENTERED CONTENT
            // ------------------------------------------
            //
            // This box has centered content.
            // We should either recurse the child box or calculate the
            // size based on the centered resource.
            if let Some(mut child) = content.el.as_ref() {
                let child_space = LayoutSpace {
                    avail_width: space.avail_width,
                    avail_height: space.avail_height,
                    children_at_this_level: 0,
                };
                let mut child_size = self.calculate_sizes(&mut child, child_space)?;
                // At this point the size of the is calculated
                // and we can determine the offset. We want to center the
                // box, so that's the center point of the parent minus
                // half the size of the child.
                //
                // The child size should have already been clipped to the available space
                child_size.l_offset.x =
                    std::cmp::max((space.avail_width / 2) - (child_size.l_size.width / 2), 0);
                child_size.l_offset.y =
                    std::cmp::max((space.avail_height / 2) - (child_size.l_size.height / 2), 0);

                ret.add_child(child_size);
            }
        }

        // ------------------------------------------
        // HANDLE THIS ELEMENT
        // ------------------------------------------
        //
        // Now that we have calculated all the children, we can handle
        // this element.
        // 1. If it has a size assigned, that is the final size, all children
        // will be clipped or scrolled inside that window.
        // 2. If no size is assigned, and we are limited in the amount of space
        // we have, then the size is available_space
        // 3. No size and no bounds means we are inside of a scrolling arena, and
        // we should grow this box to hold all of its children.
        ret.l_offset_specified = el.offset.is_some();
        if let Some(off) = el.offset {
            ret.l_offset = off;
        }

        if let Some(size) = el.size.as_ref() {
            ret.l_size = *size;
        } else {
            // first grow this box to fit its children.
            ret.resize_to_children()?;

            if ret.l_size == dom::Size::new(0, 0) {
                // if the size is still empty, there were no children. This should just be
                // sized to the available space divided by the number of
                // children.
                // Clamp to 1 to avoid dividing by zero
                let num_children = std::cmp::max(1, space.children_at_this_level);
                // TODO: add directional tiling of elements
                // for now just do vertical subdivision and fill horizontal
                ret.l_size = dom::Size::new(space.avail_width, space.avail_height / num_children);
            }

            // Then possibly clip the box by any available dimensions.
            // Add our offsets while calculating this to account for space
            // used by moving the box.

            // TODO: don't clamp, add scrolling support
            ret.l_size.width = ret
                .l_size
                .width
                .clamp(0, space.avail_width - ret.l_offset.x);
            ret.l_size.height =
                std::cmp::min(space.avail_height + ret.l_offset.y, ret.l_size.height);
        }

        log::debug!("Final size of element is {:?}", ret);

        return Ok(ret);
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
        layout: &LayoutNode,
        poffset: dom::Offset,
    ) -> Result<Option<th::Surface>> {
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
            assert!(
                (rme.rme_image.is_some() && rme.rme_color.is_none())
                    || (rme.rme_image.is_none() && rme.rme_color.is_some())
            );
            if let Some(image) = rme.rme_image.as_ref() {
                self.d_thund.bind_image(&mut surf, image.clone());
            }
            if let Some(color) = rme.rme_color.as_ref() {
                surf.set_color((color.r, color.g, color.b, color.a));
            }
            self.d_surfaces.push(surf.clone());

            // now iterate through all of it's children, and recursively do the same
            for child in layout.l_children.iter() {
                // add the new child surface as a subsurface
                let child_surf = self.create_thundr_surf_for_el(child, offset)?;
                if let Some(csurf) = child_surf {
                    surf.add_subsurface(csurf);
                }
            }

            return Ok(Some(surf));
        }

        // if we are here, then the current element does not have content.
        // Instead what we do is recursively call this function on the
        // children, and append them to the surfacelist.
        for child in layout.l_children.iter() {
            // add the new child surface as a subsurface
            self.create_thundr_surf_for_el(child, offset)?;
        }
        return Ok(None);
    }

    /// This refreshes the entire scene, and regenerates
    /// the Thundr surface list.
    pub fn refresh_elements(&mut self) -> Result<()> {
        if self.d_dom.is_none() {
            return Ok(());
        }
        let mut dom = self.d_dom.take().unwrap();

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
        let num_children = dom.layout.root_element.children.len() as u32;
        let result = self.calculate_sizes(
            &mut dom.layout.root_element,
            LayoutSpace {
                avail_width: self.d_window_dims.unwrap().0, // available width
                avail_height: self.d_window_dims.unwrap().1, // available height
                children_at_this_level: num_children,
            },
        );

        // now handle the error from our layout tree recursive call after
        // we have put the dom back
        self.d_dom = Some(dom);
        self.d_layout_tree = Some(result?);

        // reset our thundr surface list. If the set of resources has
        // changed, then we should have called clear_thundr to do so by now.
        self.d_surfaces.clear();

        // Create our thundr surface and add it to the list
        // one list with subsurfaces?
        let layout_tree = self.d_layout_tree.take().unwrap();
        let result = self.create_thundr_surf_for_el(&layout_tree, dom::Offset { x: 0, y: 0 });
        self.d_layout_tree = Some(layout_tree);

        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(e.context("Could not construct Thundr surface tree")),
        }
    }

    /// Completely flush the thundr surfaces/images and recreate the scene
    pub fn refresh_full(&mut self) -> Result<()> {
        self.clear_thundr();
        self.refresh_resource_map()?;
        self.refresh_elements()
    }

    /// Handle vulkan swapchain out of date. This is probably because the
    /// window's size has changed. This will requery the window size and
    /// refresh the layout tree.
    fn handle_ood(&mut self) -> Result<()> {
        self.d_window_dims = Some(self.d_thund.get_resolution());
        self.refresh_elements()
    }

    /// run the dakota thread.
    ///
    /// Dakota requires takover of one thread, because that's just how winit
    /// wants to work. It's annoying, but we live with it. `func` will get
    /// called before the next frame is drawn, it is the winsys event handler
    /// for the app.
    ///
    /// Returns true if we should terminate i.e. the window was closed.
    pub fn dispatch<F>(&mut self, mut func: F) -> Result<bool>
    where
        F: FnMut(),
    {
        func();
        match self.d_thund.draw_frame(&mut self.d_surfaces) {
            Ok(()) => {}
            Err(th::ThundrError::OUT_OF_DATE) => {
                self.handle_ood()?;
                return Err(Error::from(th::ThundrError::OUT_OF_DATE));
            }
            Err(e) => return Err(Error::from(e).context("Thundr: drawing failed with error")),
        };
        match self.d_thund.present() {
            Ok(()) => {}
            Err(th::ThundrError::OUT_OF_DATE) => {
                self.handle_ood()?;
                return Err(Error::from(th::ThundrError::OUT_OF_DATE));
            }
            Err(e) => return Err(Error::from(e).context("Thundr: presentation failed")),
        };

        self.d_plat.run(|| {})
    }
}
