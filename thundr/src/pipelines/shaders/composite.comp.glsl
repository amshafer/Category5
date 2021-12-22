#version 450
#extension GL_ARB_separate_shader_objects : enable
#extension GL_EXT_nonuniform_qualifier : enable
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
#define BLEND_COUNT 4
/*
 * We MUST mark this storage image as write only (aka NonReadable) or else
 * Intel decides to be big stupid and ignore writes completely. Apparently this
 * is spec compliant, but it's such a mystifying experience
 *
 * https://gitlab.freedesktop.org/mesa/mesa/-/merge_requests/10624
 */
layout (local_size_x = TILESIZE, local_size_y = TILESIZE, local_size_z = 1) in;

/* our render target: the swapchain frame to render into */
layout(binding = 0, rgba8) writeonly uniform image2D frame;

layout(binding = 2) buffer visibility_buffer
{
	ivec4 vis_buf[];
};

struct Rect {
	ivec2 start;
	ivec2 size;
};

struct Window {
	/* id.0 is the id. It is an ivec4 for alignment purposes */
	/* id.0: id that's the offset into the unbound sampler array */
	/* id.1: if we should use w_color instead of texturing */
	ivec4 id;
	/* the color used instead of texturing */
	vec4 color;
	Rect dims;
	Rect opaque;
};

/* the position/size/damage of our windows */
layout(set = 1, binding = 0, std140) buffer window_list
{
	layout(offset = 0) int window_count;
	layout(offset = 16) Window windows[];
};

/* The array of textures that are the window contents */
layout(set = 1, binding = 1) uniform sampler2D images[];

void main() {
	ivec2 uv = get_location_for_wg();
	
	/* if this invocation extends past the resolution, then do nothing */
	if(uv.x >= width || uv.y >= height)
		return;

	ivec4 target_windows = vis_buf[uv.y * width + uv.x];
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
        vec4 tex;
        /* id.1 is a flag telling us to use color */
        if (windows[target_windows[i]].id.y == 0) {
		    tex = texture(images[nonuniformEXT(windows[target_windows[i]].id.x)], win_uv);
        } else {
            tex = windows[target_windows[i]].color;
        }
		result = tex.rgb * tex.a + result * (1.0 - tex.a);
	}

	imageStore(frame, uv, vec4(result, 1.0));
}
