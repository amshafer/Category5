#version 450
#extension GL_ARB_separate_shader_objects : enable
#extension GL_GOOGLE_include_directive : enable
#include "tile_indexing.glsl"

/* Compute implementation of a compositor */

/*
Specify the size of our working groups to be 256

We will tile the display into blocks of 256 pixels whose composition
will be handled by on workgroup. AMD recommends a wg size of 256, but
it may be better to bump it up on nvidia??
*/

/* The number of windows to blend */
#define BLEND_COUNT 2
layout (local_size_x = TILESIZE, local_size_y = TILESIZE, local_size_z = 1) in;

layout(binding = 0) buffer visibility_buffer
{
	ivec4 vis_buf[];
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
	ivec2 uv = get_location_for_wg();
	
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
