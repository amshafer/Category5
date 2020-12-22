#version 450
#extension GL_ARB_separate_shader_objects : enable
#extension GL_EXT_nonuniform_qualifier : enable

/* Compute implementation of a compositor */

/*
   Specify the size of our working groups to be 256

   We will tile the display into blocks of 256 pixels whose composition
   will be handled by on workgroup. AMD recommends a wg size of 256, but
   it may be better to bump it up on nvidia??
 */

/* The width of a square tile of pixels in the screen */
#define TILESIZE 16
/* The number of windows to blend */
#define BLEND_COUNT 2
layout (local_size_x = TILESIZE, local_size_y = TILESIZE, local_size_z = 1) in;

/* our render target: the swapchain frame to render into */
layout(binding = 0, rgba32f) uniform image2D frame;

layout(binding = 1) buffer tiles
{
	int width;
	int height;
	int active_tiles[];
};

/* This is composed of window ids */
struct IdList {
	int base;
	int blend;
};

layout(binding = 2) buffer visibility_buffer
{
	IdList vis_buf[];
};

/* The array of textures that are the window contents */
layout(binding = 3) uniform sampler2D images[];

void main() {
	// TODO: remove
	vec4 r = vec4(vis_buf[0].base, vis_buf[0].base, 1.0, 1.0);
	imageStore(frame, ivec2(16, 16), r);
	imageStore(frame, ivec2(17, 16), r);
	imageStore(frame, ivec2(18, 16), r);
	imageStore(frame, ivec2(19, 16), r);
	width = 420;
	return;
	/*
	  - Get the tile for this wg from the list we initialized.
	  This tells us the base address that we are working on.
	*/
	int tile = active_tiles[gl_WorkGroupID.y * (width/TILESIZE) + gl_WorkGroupID.x];

	/*
	  - Mod the tile address by our resolution's width to get the
	  depth into a row of the framebuffer.
	  - Dividing by the width gives use the number of rows into the
	  image.
	  - Multiply them both by the tilesize to take us from the tile-grid
	  coordinate space to the pixel coordinate space
	*/
	ivec2 tile_base = ivec2(mod(tile, float(width)) * TILESIZE, (tile / width) * TILESIZE);
	/* Now index into the tile based on this invocation */
	ivec2 uv = ivec2(tile_base.x + gl_LocalInvocationID.x,
			tile_base.y + gl_LocalInvocationID.y);

	/* if this invocation extends past the resolution, then do nothing */
	if(uv.x >= width || uv.y >= height)
		return;

	ivec2 target_windows = ivec2(vis_buf[uv.y * width + uv.x].base, vis_buf[uv.y * width + uv.x].blend);
	vec3 result = vec3(0, 0, 0);
	for(int i = 0; i < BLEND_COUNT; i++) {
		if (target_windows[i] == -1)
			break;

		/*
		  For each window in the target_windows list
		  blend it into the result.
		*/
		vec4 tex = texture(images[target_windows[i]], uv);
		result = tex.rgb * tex.a + result * (1.0 - tex.a);
	}

	//imageStore(frame, uv, vec4(result, 1.0));
	imageStore(frame, uv, vec4(1.0, 1.0, 1.0, 1.0));
}
