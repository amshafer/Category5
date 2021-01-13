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
layout(binding = 0, rgba8) uniform image2D frame;

layout(binding = 1) buffer tiles
{
	int width;
	int height;
	int active_tiles[];
};

layout(binding = 2) buffer visibility_buffer
{
	ivec2 vis_buf[];
};

struct Rect {
	ivec2 start;
	ivec2 size;
};

struct Window {
	Rect dims;
	Rect opaque;
};

/* the position/size/damage of our windows */
layout(binding = 3, std140) buffer window_list
{
	layout(offset = 0) int window_count;
	layout(offset = 16) Window windows[];
};

/* The array of textures that are the window contents */
layout(binding = 4) uniform sampler2D images[];

void main() {
	/*
	  - Get the tile for this wg from the list we initialized.
	  This tells us the base address that we are working on.
	*/
	int tile = active_tiles[gl_WorkGroupID.x];

	/*
	  - Mod the tile address by our resolution's width to get the
	  depth into a row of the framebuffer.
	  - Dividing by the width gives use the number of rows into the
	  image.
	  - Multiply them both by the tilesize to take us from the tile-grid
	  coordinate space to the pixel coordinate space
	*/
	int tiles_width = (width / TILESIZE) + 1;
	ivec2 tile_base = ivec2(mod(tile, tiles_width) * TILESIZE, (tile / tiles_width) * TILESIZE);
	/* Now index into the tile based on this invocation */
	ivec2 uv = ivec2(tile_base.x + gl_LocalInvocationID.x,
			tile_base.y + gl_LocalInvocationID.y);

	/* if this invocation extends past the resolution, then do nothing */
	if(uv.x >= width || uv.y >= height)
		return;

	ivec2 target_windows = vis_buf[uv.y * width + uv.x];
	vec3 result = vec3(0, 0, 0);
	for(int i = 0; i < BLEND_COUNT; i++) {
		if (target_windows[i] == -1)
			continue;

		/*
		  We can't use the uv coordinates because they are the index
		  into the frame. We need to subtract the offset of the
		  windows current position to find window-coordinates
		*/
		ivec2 ws_raw = ivec2(0, 0);
		ws_raw = uv - windows[target_windows[i]].dims.start;

		/* bound the base offset to be within the screen size */
		//if (windows[i].dims.start.x >= 0 && windows[i].dims.start.y >= 0)
		//	ws_raw = uv - windows[i].dims.start;
		//if (ws_raw.x > width)
		//	ws_raw.x = width;
		//if (ws_raw.y > height)
			//ws_raw.y = height;

		/*
		  Now we need to adjust the image dimensions based on the
		  image size. We need to transfer from screen coords to image coords.
		  Image coords are normalized from [0,1]
		*/
		vec2 win_uv = vec2(
			float(ws_raw.x) / float(windows[target_windows[i]].dims.size.x),
			float(ws_raw.y) / float(windows[target_windows[i]].dims.size.y)
		);

		/*
		  For each window in the target_windows list
		  blend it into the result.
		*/
		vec4 tex = texture(images[nonuniformEXT(target_windows[i])], win_uv);
		result = tex.rgb * tex.a + result * (1.0 - tex.a);
	}

	//int i = 1;
	//if (uv.x >= windows[i].dims.size.x || uv.y >= windows[i].dims.size.y)
	//	return;
	//vec2 win_uv = vec2(
	//		float(uv.x) / float(windows[i].dims.size.x),
	//		float(uv.y) / float(windows[i].dims.size.y)
	//		);
	//result = texture(images[i], win_uv).rgb;

	imageStore(frame, uv, vec4(result, 1.0));
}
