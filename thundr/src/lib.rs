// The Thundr renderer
//
// Austin Shafer - 2020

mod descpool;
mod display;
mod renderer;
mod damage;
mod list;
mod image;
mod surface;
mod pipelines;

pub use list::SurfaceList;
pub use renderer::Renderer;
pub use image::Image;
pub use surface::Surface;

use renderer::RendererCreateInfo;

#[macro_use]
extern crate memoffset;
extern crate utils;
use crate::utils::{MemImage,Dmabuf};
use pipelines::*;

pub struct Thundr {
    th_rend: Renderer,

    /// Application specific stuff that will be set up after
    /// the original initialization
    pub(crate) th_pipe: Box<dyn Pipeline>,
}

// This is the public facing thundr api. Don't change it
impl Thundr {

    // TODO: make get_available_params and add customization
    pub fn new() -> Thundr {
        // creates a context, swapchain, images, and others
        // initialize the pipeline, renderpasses, and display engine
        let info = RendererCreateInfo {
            enabled_pipelines: vec! { PipelineType::COMPUTE },
        };
        let mut rend = Renderer::new(&info);
        let pipe = Box::new(CompPipeline::new(&mut rend));

        Thundr {
            th_rend: rend,
            th_pipe: pipe,
        }
    }

    pub fn get_resolution(&self) -> (u32, u32) {
        (self.th_rend.resolution.width,
         self.th_rend.resolution.height)
    }

    // create_image_from_bits
    pub fn create_image_from_bits(&mut self,
                                  img: &MemImage,
                                  release_info: Option<Box<dyn Drop>>)
                                  -> Option<Image>
    {
        self.th_rend.create_image_from_bits(
            &img, release_info,
        )
    }

    // create_image_from_dmabuf
    pub fn create_image_from_dmabuf(&mut self,
                                    dmabuf: &Dmabuf,
                                    release_info: Option<Box<dyn Drop>>)
                                    -> Option<Image>
    {
        self.th_rend.create_image_from_dmabuf(
            dmabuf, release_info,
        )
    }

    pub fn destroy_image(&mut self, image: Image) {
        self.th_rend.destroy_image(&image);
    }

    pub fn update_image_from_bits(&mut self,
                                  image: &mut Image,
                                  memimg: &MemImage,
                                  release_info: Option<Box<dyn Drop>>)
    {
        self.th_rend.update_image_from_bits(
            image, memimg, release_info,
        )
    }

    // create_image_from_dmabuf
    pub fn update_image_from_dmabuf(&mut self,
                                    image: &mut Image,
                                    dmabuf: &Dmabuf,
                                    release_info: Option<Box<dyn Drop>>)
    {
        self.th_rend.update_image_from_dmabuf(
            image, dmabuf, release_info,
        )
    }

    /// Creates a new surface.
    ///
    /// A surface represents a geometric region that will be
    /// drawn. It needs to have an image attached. The same
    /// image can be bound to multiple surfaces.
    pub fn create_surface(&mut self,
                          x: f32,
                          y: f32,
                          width: f32,
                          height: f32)
                          -> Surface
    {
        Surface::create_surface(x, y, width, height)
    }

    /// Attaches an image to this surface, when this surface
    /// is drawn the contents will be sample from `image`
    pub fn bind_image(&self, surf: &mut Surface, image: Image) {
        surf.bind_image(image);
    }

    // release_pending_resources
    pub fn release_pending_resources(&mut self) {
        self.th_rend.release_pending_resources();
    }

    // draw_frame
    pub fn draw_frame(&mut self, surfaces: &SurfaceList) {
        // record rendering commands
        let params = self.th_rend.begin_recording_one_frame(surfaces);
        self.th_pipe.draw(&self.th_rend, &params, surfaces);
    }

    // present
    pub fn present(&mut self) {
        self.th_rend.present();
    }
}

impl Drop for Thundr {
    fn drop(&mut self) {
        // first destroy the pipeline specific resources
        self.th_pipe.destroy(&mut self.th_rend);
        // th_rend will now be dropped
    }
}
