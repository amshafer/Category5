#version 450
#extension GL_ARB_separate_shader_objects : enable
#extension GL_EXT_nonuniform_qualifier : enable

layout(location = 0) in vec2 coord;
layout(location = 1) flat in int window_index;
layout(location = 0) out vec4 res;

layout(set = 1, binding = 1) uniform sampler2D tex;

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
	if (windows[window_index].id.y > 0) {
		res = windows[window_index].color;
	} else {
		res = texture(images[nonuniformEXT(windows[window_index].id.x)], coord);
	}
}
