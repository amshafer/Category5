#version 450
#extension GL_ARB_separate_shader_objects : enable

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

layout(binding = 0) buffer visibility_buffer
{
	ivec4 vis_buf[];
};

/* the position/size/damage of our windows */
layout(binding = 1) buffer tiles
{
	int width;
	int height;
	int active_tiles[];
};

struct Rect {
	ivec2 start;
	ivec2 size;
};

struct Window {
	/* id.0 is the id. It is an ivec4 for alignment purposes */
	ivec4 id;
	Rect dims;
	Rect opaque;
};

/* the position/size/damage of our windows */
layout(binding = 2, std140) buffer window_list
{
	layout(offset = 0) int window_count;
	layout(offset = 16) Window windows[];
};

/*
  Does the opaque region of window at index i contain the point (x, y)
*/
bool opaque_contains(int i, ivec2 uv) {
	return all(greaterThanEqual(uv, windows[i].dims.start))
	&& all(lessThan(uv, windows[i].dims.start + windows[i].dims.size));
}

/*
  Does the window at index i contain the point (x, y)
*/
bool contains(int i, ivec2 uv) {
	return all(greaterThanEqual(uv, windows[i].dims.start))
	&& all(lessThan(uv, windows[i].dims.start + windows[i].dims.size));
}

void main() {
	/*
	  - Get the tile for this wg from the list we initialized.
	  This tells us the base address that we are working on.
	*/
	int tile = active_tiles[gl_WorkGroupID.x];

	/*
	  - Mod the tile address by our resolution's width (in tiles) to get the
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

	ivec4 result = ivec4(-1, -1, -1, -1);
	/* This is the current index into result we are calculating */
	int idx = 3;
	for(int i = 0; i < window_count; i++) {
		/* TODO: test for intersection */
		if (windows[i].opaque.start.x != -1 && opaque_contains(i, uv)) {
			/* we found a non-blending matching pixel, so exit */
			result[idx] = i;
			break;

		} else if (contains(i, uv)) {
			/*
			  we found a potentially transparent portion of the
			  window containing this pixel, so keep going to
			  collect the list of other windows to blend with
			*/
			result[idx] = i;
			if (idx < 0)
				break;
			idx--;
		}
	}

	/* Write our window ids to the visibility buffer */
	vis_buf[uv.y * width + uv.x] = result;
}
