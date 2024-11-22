// The Display object owned by Renderer
//
// Austin Shafer - 2020

extern crate ash;

use ash::extensions::khr;
use ash::vk;

use crate::device::Device;
use crate::pipelines::*;
use crate::*;

use std::sync::Arc;

mod vkswapchain;
use vkswapchain::VkSwapchain;
mod headless;
use headless::HeadlessSwapchain;
pub mod frame;
use frame::{FrameRenderer, RecordParams};

#[cfg(feature = "drm")]
pub mod drm;

/// Shared state that subsystems consume. We need this
/// since Display holds rendering objects, but also has
/// to pass down swapchain/image info so those rendering
/// objects can access them
pub struct DisplayState {
    /// a set of images belonging to swapchain
    pub(crate) d_images: Vec<vk::Image>,
    /// views describing how to access the images
    ///
    /// The Pipeline depends on these, so
    /// when they are changed those resources will also
    /// have to be generated.
    pub(crate) d_views: Vec<vk::ImageView>,
    /// Current resolution of this output
    pub d_resolution: vk::Extent2D,
    // Vulkan surface capabilities
    pub d_surface_caps: vk::SurfaceCapabilitiesKHR,
    pub d_surface_format: vk::SurfaceFormatKHR,
    /// index into swapchain images that we are currently using
    pub(crate) d_current_image: u32,
    /// Headless backend does not need a present sema
    pub(crate) d_needs_present_sema: bool,
    /// These semaphores control access to d_images and signal
    /// when they can be modified after vkAcquireNextImageKHR.
    /// They will be moved from the available list and populated
    /// for an image index when used
    pub(crate) d_present_semas: Vec<Option<vk::Semaphore>>,
    /// These are the currently unused semas for image acquire
    pub(crate) d_available_present_semas: Vec<vk::Semaphore>,
    /// processes things to be physically displayed
    pub(crate) d_present_queue: vk::Queue,
    /// The cached graphics queue family index we prefer
    pub(crate) d_graphics_queue_family: u32,
    /// Frame end semaphore
    pub(crate) d_frame_sema: vk::Semaphore,
}

/// A display represents a physical screen
///
/// This is mostly the same as vulkan's concept of a display,
/// but it is a bit different. This name is overloaded as vulkan,
/// ash, and us have something called a display. Essentially
/// this holds the PFN loaders, the display KHR extension object,
/// and the surface generated for the physical display.
///
/// The swapchain is generated (and regenerated) from this stuff.
pub struct Display {
    pub d_dev: Arc<Device>,
    /// Our swapchain of images. This holds the different backends
    d_swapchain: Box<dyn Swapchain>,
    /// State to share with Renderer
    pub(crate) d_state: DisplayState,
    /// Application specific stuff that will be set up after
    /// the original initialization
    pub(crate) d_pipe: GeomPipeline,
}

/// Our Swapchain Backend
///
/// A swapchain is a collection of images that we will use to represent
/// the frames in our presentation. These swapchains may have multiple
/// types of implementations, headless, DRM-based, Vulkan Direct 2 display.
pub(crate) trait Swapchain {
    /// Choose a queue family
    ///
    /// returns an index into the array of queue types.
    /// provide the surface PFN loader and the surface so
    /// that we can ensure the pdev/queue combination can
    /// present the surface
    fn select_queue_family(&self) -> Result<u32>;

    /// Get the surface information
    ///
    /// These capabilities are used elsewhere to identify swapchain
    /// surface capabilities. Even if the swapchain doesn't actually
    /// use VkSurfaceKHR these will still be filled in.
    fn get_surface_info(&self) -> Result<(vk::SurfaceCapabilitiesKHR, vk::SurfaceFormatKHR)>;

    /// Recreate our swapchain.
    ///
    /// This will be done on VK_ERROR_OUT_OF_DATE_KHR, signifying that
    /// the window is being resized and we have to regenerate accordingly.
    /// Keep in mind the Pipeline in Thundr will also have to be recreated
    /// separately.
    fn recreate_swapchain(&mut self, dstate: &mut DisplayState) -> Result<()>;

    /// Get the Dots Per Inch for this display.
    ///
    /// For VK_KHR_display we will calculate it ourselves, and for
    /// SDL we will ask SDL to tell us it.
    fn get_dpi(&self) -> Result<(i32, i32)>;

    /// Update self.current_image with the swapchain image to render to
    ///
    /// If the next image is not ready (i.e. if Vulkan returned NOT_READY or
    /// TIMEOUT), then this will loop on calling `vkAcquireNextImageKHR` until
    /// it gets a valid image. This has to be done on AMD hw or else the TIMEOUT
    /// error will get passed up the callstack and fail.
    fn get_next_swapchain_image(&mut self, dstate: &mut DisplayState) -> Result<()>;

    /// Present the current swapchain image to the screen.
    ///
    /// Finally we can actually flip the buffers and present
    /// this image.
    fn present(&mut self, dstate: &DisplayState) -> Result<()>;
}

impl Display {
    /// Figure out what the requested surface type is and call the appropriate
    /// swapchain backend new function.
    fn initialize_swapchain(info: &CreateInfo, dev: Arc<Device>) -> Result<Box<dyn Swapchain>> {
        match &info.surface_type {
            #[cfg(feature = "sdl")]
            SurfaceType::SDL2(_, _) => Ok(Box::new(VkSwapchain::new(info, dev.clone())?)),
            SurfaceType::Display(_) => Ok(Box::new(VkSwapchain::new(info, dev.clone())?)),
            SurfaceType::Headless => Ok(Box::new(HeadlessSwapchain::new(dev.clone())?)),
            #[cfg(feature = "drm")]
            SurfaceType::Drm => Ok(Box::new(drm::DrmSwapchain::new(dev.clone())?)),
        }
    }

    pub fn new(info: &CreateInfo, dev: Arc<Device>, tmp_image: Image) -> Result<Display> {
        unsafe {
            let swapchain = Self::initialize_swapchain(info, dev.clone())?;
            let queue_family = swapchain.select_queue_family()?;

            // Ensure that there is a valid queue, validation layer checks for this
            let graphics_queue_family = swapchain.select_queue_family()?;
            let present_queue = dev.dev.get_device_queue(graphics_queue_family, 0);

            let sema_create_info = vk::SemaphoreCreateInfo::default();
            let frame_sema = dev.dev.create_semaphore(&sema_create_info, None).unwrap();

            let (surface_caps, surface_format) = swapchain.get_surface_info()?;
            let dstate = DisplayState {
                d_surface_caps: surface_caps,
                d_surface_format: surface_format,
                d_resolution: vk::Extent2D {
                    width: 0,
                    height: 0,
                },
                d_views: Vec::with_capacity(0),
                d_current_image: 0,
                d_needs_present_sema: match info.surface_type {
                    SurfaceType::Headless => false,
                    #[cfg(feature = "drm")]
                    SurfaceType::Drm => false,
                    _ => true,
                },
                d_present_semas: Vec::new(),
                d_available_present_semas: Vec::new(),
                d_present_queue: present_queue,
                d_frame_sema: frame_sema,
                d_graphics_queue_family: queue_family,
                d_images: Vec::with_capacity(0),
            };

            let pipe = GeomPipeline::new(dev.clone(), &dstate, tmp_image)?;

            let mut ret = Self {
                d_dev: dev,
                d_swapchain: swapchain,
                d_state: dstate,
                d_pipe: pipe,
            };

            // Trigger the creation of our swapchain images and pipeline framebuffers
            ret.handle_ood()?;

            Ok(ret)
        }
    }

    /// Destroy the swapchain bits in dstate
    fn destroy_swapchain_resources(&mut self) {
        unsafe {
            self.d_dev.dev.device_wait_idle().unwrap();

            // Don't destroy the images here, the destroy swapchain call
            // will take care of them
            for view in self.d_state.d_views.iter() {
                self.d_dev.dev.destroy_image_view(*view, None);
            }
            self.d_state.d_views.clear();

            for sema in self.d_state.d_present_semas.drain(..) {
                if let Some(sema) = sema {
                    self.d_dev.dev.destroy_semaphore(sema, None);
                }
            }
            for sema in self.d_state.d_available_present_semas.drain(..) {
                self.d_dev.dev.destroy_semaphore(sema, None);
            }
        }
    }

    /// Recreate our swapchain.
    ///
    /// This will be done on VK_ERROR_OUT_OF_DATE_KHR, signifying that
    /// the window is being resized and we have to regenerate accordingly.
    /// Keep in mind the Pipeline in Thundr will also have to be recreated
    /// separately.
    pub fn recreate_swapchain(&mut self) -> Result<()> {
        self.destroy_swapchain_resources();

        self.d_swapchain.recreate_swapchain(&mut self.d_state)?;

        // Populate the present semas for these images
        let sema_create_info = vk::SemaphoreCreateInfo::default();

        // Have an extra semaphore since we need one to use but don't
        // know which images are done yet
        for _ in 0..(self.d_state.d_images.len() + 1) {
            // Don't create present semas for certain backends. Things
            // like headless don't do presentation and therefore don't
            // need this synchronization
            if self.d_state.d_needs_present_sema {
                self.d_state.d_available_present_semas.push(unsafe {
                    self.d_dev
                        .dev
                        .create_semaphore(&sema_create_info, None)
                        .unwrap()
                });
            }
            self.d_state.d_present_semas.push(None);
        }

        Ok(())
    }

    /// This is a candidate for an out of date error. We should
    /// let the application know about this so it can recalculate anything
    /// that depends on the window size, so we exit returning OOD.
    ///
    /// We have to destroy and recreate our pipeline along the way since
    /// it depends on the swapchain.
    pub fn handle_ood(&mut self) -> Result<()> {
        self.recreate_swapchain()?;
        self.d_pipe.handle_ood(&mut self.d_state);

        Ok(())
    }

    /// Get the DRM device major/minor in use by this Display's Device
    pub fn get_drm_dev(&self) -> (i64, i64) {
        self.d_dev.get_drm_dev()
    }

    /// Get the Dots Per Inch for this display.
    ///
    /// For VK_KHR_display we will calculate it ourselves, and for
    /// SDL we will ask SDL to tell us it.
    pub fn get_dpi(&self) -> Result<(i32, i32)> {
        self.d_swapchain.get_dpi()
    }

    /// Get the resolution of this display
    ///
    /// This returns the extent as used by Vulkan
    pub fn get_resolution(&self) -> (u32, u32) {
        (
            self.d_state.d_resolution.width,
            self.d_state.d_resolution.height,
        )
    }

    /// Get a list of any extension names needed by the Vulkan
    /// extensions in use by this Display.
    pub fn extension_names(info: &CreateInfo) -> Vec<*const i8> {
        match info.surface_type {
            SurfaceType::Headless => Vec::with_capacity(0),
            #[cfg(feature = "drm")]
            SurfaceType::Drm => Vec::with_capacity(0),
            SurfaceType::Display(_) => {
                vec![khr::Surface::name().as_ptr(), khr::Display::name().as_ptr()]
            }
            #[cfg(feature = "sdl")]
            SurfaceType::SDL2(_, win) => win
                .vulkan_instance_extensions()
                .unwrap()
                .iter()
                .map(|s| {
                    // we need to turn a Vec<&str> into a Vec<*const i8>
                    s.as_ptr() as *const i8
                })
                .collect(),
        }
    }

    /// Update self.current_image with the swapchain image to render to
    ///
    /// If the next image is not ready (i.e. if Vulkan returned NOT_READY or
    /// TIMEOUT), then this will loop on calling `vkAcquireNextImageKHR` until
    /// it gets a valid image. This has to be done on AMD hw or else the TIMEOUT
    /// error will get passed up the callstack and fail.
    pub fn get_next_swapchain_image(&mut self) -> Result<()> {
        self.d_swapchain.get_next_swapchain_image(&mut self.d_state)
    }

    /// Begin recording a frame
    ///
    /// This is first called when trying to draw a frame. It will set
    /// up the command buffers and resources that Thundr will use while
    /// recording draw commands.
    pub fn acquire_next_frame<'a>(&'a mut self) -> Result<FrameRenderer<'a>> {
        // Before waiting for the latest frame, free the previous
        // frame's release data
        self.d_dev.flush_deletion_queue();

        // Get our next swapchain image
        match self.get_next_swapchain_image() {
            Ok(()) => (),
            Err(ThundrError::OUT_OF_DATE) => {
                self.handle_ood()?;
                return Err(ThundrError::OUT_OF_DATE);
            }
            Err(e) => return Err(e),
        };

        // Wait for the previous frame to finish, preventing us from having the
        // CPU run ahead more than one frame.
        //
        // This throttling helps reduce latency, as we don't queue up more than
        // one frame at a time. With this we get one frame (16ms) latency.
        //
        // TODO: pace our frames better to reduce latency futher?
        self.d_dev.wait_for_latest_timeline();

        // Now construct our FrameRenderer
        // This allows the caller to have
        let res = self.get_resolution();
        let mut params = RecordParams::new(&self.d_dev);
        params.push.width = res.0;
        params.push.height = res.1;

        // Kick off our new frame
        self.d_pipe.begin_record(&self.d_state);

        let frame = FrameRenderer {
            fr_swapchain: &mut self.d_swapchain,
            fr_dstate: &self.d_state,
            fr_pipe: &mut self.d_pipe,
            fr_params: params,
        };

        Ok(frame)
    }

    /// Get the content of the current swapchain image
    ///
    /// Keep in mind that this will be very expensive and synchronized. It
    /// also should be done before the next image is acquired.
    #[allow(dead_code)]
    pub fn dump_framebuffer(&mut self, filename: &str) -> MappedImage {
        // alloc a temp image
        let (image, view, mem) = self.d_dev.create_image(
            &self.d_state.d_resolution,
            vk::Format::B8G8R8A8_UNORM,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
            vk::ImageAspectFlags::COLOR,
            vk::MemoryPropertyFlags::DEVICE_LOCAL
                | vk::MemoryPropertyFlags::HOST_COHERENT
                | vk::MemoryPropertyFlags::HOST_VISIBLE,
            vk::ImageTiling::LINEAR,
        );

        let present_layout = match self.d_state.d_needs_present_sema {
            true => vk::ImageLayout::PRESENT_SRC_KHR,
            false => vk::ImageLayout::GENERAL,
        };

        // Wait for both the latest frame and for the copy cbuf
        self.d_dev.wait_for_latest_timeline();
        self.d_dev.wait_for_copy();

        unsafe {
            let int_lock = self.d_dev.d_internal.clone();
            let internal = int_lock.write().unwrap();

            self.d_dev.cbuf_begin_recording(
                internal.copy_cbuf,
                vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
            );

            let range = vk::ImageSubresourceRange::builder()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .layer_count(1)
                .level_count(1)
                .build();

            // transition our tmp image to TRANSFER_DST
            let tmp_src = vk::ImageMemoryBarrier::builder()
                .image(image)
                .src_access_mask(vk::AccessFlags::default())
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .subresource_range(range)
                .build();

            // transition our swapchain image to TRANSFER_SRC
            let swapchain_src = vk::ImageMemoryBarrier::builder()
                .image(self.d_state.d_images[self.d_state.d_current_image as usize])
                .src_access_mask(vk::AccessFlags::MEMORY_READ)
                .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
                .old_layout(present_layout)
                .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .subresource_range(range)
                .build();
            self.d_dev.dev.cmd_pipeline_barrier(
                internal.copy_cbuf,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[tmp_src, swapchain_src],
            );

            // copy from the swapchain image
            let image_copy = vk::ImageCopy::builder()
                .src_subresource(
                    vk::ImageSubresourceLayers::builder()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .layer_count(1)
                        .build(),
                )
                .dst_subresource(
                    vk::ImageSubresourceLayers::builder()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .layer_count(1)
                        .build(),
                )
                .extent(self.d_state.d_resolution.into())
                .build();

            self.d_dev.dev.cmd_copy_image(
                internal.copy_cbuf,
                self.d_state.d_images[self.d_state.d_current_image as usize],
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[image_copy],
            );

            // transition our tmp image to general
            let tmp_dst = vk::ImageMemoryBarrier::builder()
                .image(image)
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::MEMORY_READ)
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::GENERAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .subresource_range(range)
                .build();

            // transition the swapchain image back to optimal
            let swapchain_dst = vk::ImageMemoryBarrier::builder()
                .image(self.d_state.d_images[self.d_state.d_current_image as usize])
                .src_access_mask(vk::AccessFlags::TRANSFER_READ)
                .dst_access_mask(vk::AccessFlags::MEMORY_READ)
                .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                .new_layout(present_layout)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .subresource_range(range)
                .build();
            self.d_dev.dev.cmd_pipeline_barrier(
                internal.copy_cbuf,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[tmp_dst, swapchain_dst],
            );

            self.d_dev.cbuf_end_recording(internal.copy_cbuf);
        }

        self.d_dev.copy_cbuf_submit_async();
        self.d_dev.wait_for_copy();

        unsafe {
            // get image layout
            let sublayout = self.d_dev.dev.get_image_subresource_layout(
                image,
                vk::ImageSubresource::builder()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .build(),
            );

            // Map our tmp image's memory
            let ptr = self
                .d_dev
                .dev
                .map_memory(
                    mem,
                    sublayout.offset,
                    sublayout.size,
                    vk::MemoryMapFlags::empty(),
                )
                .unwrap();

            // copy our image data from the tmp image to an array
            let data =
                std::slice::from_raw_parts_mut(ptr as *mut u8, sublayout.size as usize).to_vec();

            self.d_dev.dev.unmap_memory(mem);

            // Clean up our tmp image
            self.d_dev.dev.destroy_image(image, None);
            self.d_dev.dev.destroy_image_view(view, None);
            self.d_dev.free_memory(mem);

            // dump our data to a ppm file
            {
                use std::io::Write;

                let mut f = std::fs::File::create(filename).unwrap();
                // write ppm header
                f.write(
                    format!(
                        "P6\n{}\n{}\n255\n",
                        self.d_state.d_resolution.width, self.d_state.d_resolution.height
                    )
                    .as_bytes(),
                )
                .unwrap();
                // write pixel data
                for pixel in data.as_slice().chunks(4) {
                    // swizzle to RGB format
                    f.write(&[pixel[2]]).unwrap();
                    f.write(&[pixel[1]]).unwrap();
                    f.write(&[pixel[0]]).unwrap();
                }
            }

            MappedImage { mi_data: data }
        }
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        println!("Destroying display");
        unsafe {
            self.d_dev.dev.device_wait_idle().unwrap();
            self.destroy_swapchain_resources();
            self.d_dev
                .dev
                .destroy_semaphore(self.d_state.d_frame_sema, None);
        }
    }
}
