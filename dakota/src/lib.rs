extern crate image;
extern crate serde;
extern crate thundr as th;

extern crate utils;
pub use utils::{anyhow, region::Rect, Context, MemImage, Result};

pub mod dom;
use dom::DakotaDOM;
mod platform;
use platform::Platform;
pub mod xml;

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;

pub struct Dakota {
    #[cfg(feature = "xcb")]
    d_plat: platform::XCBPlat,
    #[cfg(feature = "macos")]
    d_plat: platform::MacosPlat,
    d_thund: th::Thundr,
    d_resmap: HashMap<String, th::Image>,
    d_surfaces: th::SurfaceList,
    d_dom: Option<DakotaDOM>,
}

impl Dakota {
    /// Construct a new Dakota instance
    ///
    /// This will initialize the window system platform layer, create a thundr
    /// instance from it, and wrap it in Dakota.
    pub fn new() -> Result<Self> {
        #[cfg(feature = "wayland")]
        let mut plat = platform::WLPlat::new()?;

        #[cfg(feature = "macos")]
        let mut plat = platform::MacosPlat::new()?;

        #[cfg(feature = "xcb")]
        let mut plat = platform::XCBPlat::new()?;

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
            if let Some(image) = res.image.as_ref() {
                if image.format != dom::Format::ARGB8888 {
                    return Err(anyhow!("Invalid image format"));
                }

                let file_path = res.data.get_fs_path()?;
                let file = File::open(file_path)?;
                let file_reader = BufReader::new(file);

                let ireader = image::io::Reader::new(file_reader)
                    .with_guessed_format()
                    .context("Could not open image specified in Dakota spec")?;

                // Create an in-memory representation of the image contents
                let resolution = image::image_dimensions(std::path::Path::new(file_path)).context(
                    "Format of image could not be guessed correctly. Could not get resolution",
                )?;
                let image_data = ireader
                    .decode()
                    .context("Could not decode image")?
                    .to_bgra8()
                    .into_vec();
                let mimg = MemImage::new(
                    image_data.as_slice().as_ptr() as *mut u8,
                    4,                     // width of a pixel
                    resolution.0 as usize, // width of texture
                    resolution.1 as usize, // height of texture
                );

                // create a thundr image for each resource
                let th_image = self.d_thund.create_image_from_bits(&mimg, None).unwrap();

                // Add the new image to our resource map
                self.d_resmap.insert(res.name.clone(), th_image);
            }
        }
        Ok(())
    }

    /// Get the minimum size that a resource wants.
    ///
    /// This is used to scale boxes larger than the requirements of the children.
    pub fn get_resource_size(&mut self, _res: &String) -> Result<dom::Size> {
        // TODO: look up resource size in map
        Err(anyhow!(""))
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
    pub fn calculate_sizes(
        &mut self,
        el: &mut dom::Element,
        mut available_width: Option<u32>,
        mut available_height: Option<u32>,
    ) -> Result<()> {
        assert!(
            (el.children.len() > 0 && el.content.is_none())
                || (el.children.len() == 0 && el.content.is_some())
        );

        // check if this element has its size set, shrink the available space
        // to match.
        if let Some(size) = el.size.as_ref() {
            available_width = Some(size.width);
            available_height = Some(size.height);
        }

        // if the box has children, then recurse through them and calculate our
        // box size based on the fill type.
        if el.children.len() > 0 {
            for child in el.children.iter_mut() {
                self.calculate_sizes(child, available_width, available_height)?;
            }
        } else {
            // This box has centered content.
            // We should either recurse the child box or calculate the
            // size based on the centered resource.
            let content = el.content.as_mut().unwrap();
            if let Some(mut child) = content.el.as_mut() {
                self.calculate_sizes(&mut child, available_width, available_height)?;
            }
        }

        // Now that we have calculated all the children, we can handle
        // this element.
        // 1. If it has a size assigned, that is the final size, all children
        // will be clipped or scrolled inside that window.
        // 2. If no size is assigned, and we are limited in the amount of space
        // we have, then the size is available_space
        // 3. No size and no bounds means we are inside of a scrolling arena, and
        // we should grow this box to hold all of its children.

        if el.size.is_none() {
            // first grow this box to fit its children.
            el.resize_to_children()?;

            // if the size is still empty, there were no children. This should just be
            // sized to the available space
            if el.size.is_none() {
                // The default size is based on the resource's default size.
                // No size + no resource + no bounds means we default to size
                // of 0.
                el.size = match el.resource.as_ref() {
                    Some(res) => Some(self.get_resource_size(&res)?),
                    None => None,
                };
            }

            // Then clip the box by any available dimensions
            if let Some(size) = el.size.as_mut() {
                if let Some(width) = available_width {
                    size.width = std::cmp::min(width, size.width);
                }
                if let Some(height) = available_height {
                    size.height = std::cmp::min(height, size.height);
                }
            }
        }

        return Ok(());
    }

    /// This refreshes the entire scene
    pub fn refresh_elements(&mut self) -> Result<()> {
        self.d_surfaces.clear();

        if self.d_dom.is_none() {
            return Ok(());
        }
        let mut dom = self.d_dom.take().unwrap();

        // TODO: construct layout tree with sizes of all boxes
        // create our thundr surfaces while we are at it.
        let result = self.calculate_sizes(
            &mut dom.layout.root_element,
            Some(dom.window.width),  // available width
            Some(dom.window.height), // available height
        );

        self.d_dom = Some(dom);

        // now handle the error from our layout tree recursive call after
        // we have put the dom back
        result?;

        // TODO: Create our thundr surface and add it to the list
        // one list with subsurfaces?

        Ok(())
    }

    /// Completely flush the thundr surfaces/images and recreate the scene
    pub fn refresh_full(&mut self) -> Result<()> {
        self.refresh_resource_map()?;
        self.refresh_elements()?;

        //TODO: enable me after adding swapchain reconstruction
        //let dom = match &mut self.d_dom {
        //    Some(dom) => dom,
        //    None => {
        //        return Err(anyhow!(
        //            "A scene is not loaded in Dakota. Please load one from xml",
        //        ))
        //    }
        //};
        //self.d_plat.set_output_params(&dom.window)
        Ok(())
    }

    /// run the dakota thread.
    ///
    /// Dakota requires takover of one thread, because that's just how winit
    /// wants to work. It's annoying, but we live with it. `func` will get
    /// called before the next frame is drawn, it is the winsys event handler
    /// for the app.
    pub fn dispatch<F>(&mut self, mut func: F) -> Result<()>
    where
        F: FnMut(),
    {
        let plat = &mut self.d_plat;
        let thund = &mut self.d_thund;
        let surfs = &mut self.d_surfaces;

        plat.run(|| {
            func();
            thund.draw_frame(surfs);
            thund.present();
        })
    }
}
