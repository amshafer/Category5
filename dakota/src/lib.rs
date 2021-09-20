extern crate image;
extern crate serde;
extern crate thundr as th;

extern crate utils;
pub use utils::{anyhow, Context, MemImage, Result};

pub mod dom;
use dom::DakotaDOM;
mod platform;
use platform::Platform;
pub mod xml;

use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
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

    pub fn refresh_elements(&mut self) -> Result<()> {
        let dom = match &mut self.d_dom {
            Some(dom) => dom,
            None => {
                return Err(anyhow!(
                    "A scene is not loaded in Dakota. Please load one from xml",
                ))
            }
        };
        self.d_surfaces.clear();

        // TODO: construct layout tree with sizes of all boxes

        // TODO: create (update?) a thundr surface for each box
        for el in dom.layout.elements.iter() {
            assert!(
                (el.children.len() > 0 && el.content.is_none())
                    || (el.children.len() == 0 && el.content.is_some())
            );

            // make a thundr surface for each element
            let mut surf = self.d_thund.create_surface(0.0, 0.0, 512.0, 512.0);
            if let Some(content) = el.content.as_ref() {
                // TODO: recurse here for box contents
                if let Some(res) = content.resource.as_ref() {
                    // look up and bind to the image specified by <resource>
                    let th_image = self.d_resmap.get(res).context(
                        "Could not find resource. Please specify it as part of resourceMap",
                    )?;
                    self.d_thund.bind_image(&mut surf, th_image.clone());
                }
            }

            self.d_surfaces.push(surf);
        }
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
