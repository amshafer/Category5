# Thundr

Thundr is a Vulkan composition library for use in ui toolkits and
wayland compositors. You use it to create a set of images from
textures or window contents, attach those images to surfaces, and pass
a list of surfaces to thundr for rendering.

Thundr also supports multiple methods of drawing:
* `compute` - Uses compute shaders to perform compositing.
* `geometric` - This is a more "traditional" manner of drawing ui elements:
surfaces are drawn as textured quads in 3D space.

The compute pipeline is more optimized, and is the default. The
geometric pipeline serves as a backup for situations in which the
compute pipeline does not perform well or is not supported.

## Drawing API

The general flow of a thundr client is as follows:
* Create an Image (`create_image_from*`)
  * Use a MemImage to load a texture from raw bits.
  * Use a dmabuf to load a image contents from a gpu buffer.
* Create a Surface (`create_surface`)
  * Assign it a location and a size
* Bind the image to the surface (`bind_image`)
* Create a surface list (`SurfaceList::new()`)
  * Push the surfaces you'd like rendered into the list from front to
  back (`SurfaceList.push`)
* Tell Thundr to launch the work on the gpu (`draw_frame`)
* Present the rendering results on screen (`present`)

```
use thundr as th;

let thund: th::Thundr = Thundr::new();

// First load our texture into memory
let img = image::open("images/cursor.png").unwrap().to_rgba();
let pixels: Vec<u8> = img.into_vec();
let mimg = MemImage::new(
    pixels.as_slice().as_ptr() as *mut u8,
    4,  // width of a pixel
    64, // width of texture
    64  // height of texture
);

// Create an image from our MemImage
let image = thund.create_image_from_bits(&mimg, None).unwrap();
// Now create a 16x16 surface at position (0, 0)
let mut surf = thund.create_surface(0.0, 0.0, 16.0, 16.0);
// Assign our image to our surface
thund.bind_image(&mut surf, image);
```



## Requirements

Thundr requires a system with vulkan 1.2+ installed. The following
extensions are used:
* VK_KHR_surface
* VK_KHR_display
* VK_EXT_maintenance2
* VK_KHR_debug_report
* VK_KHR_descriptor_indexing
* VK_KHR_external_memory
