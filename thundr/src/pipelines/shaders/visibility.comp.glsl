#version 450
#extension GL_ARB_separate_shader_objects : enable

/* Compute implementation of a compositor */

/*
Specify the size of our working groups to be 256

We will tile the display into blocks of 256 pixels whose composition
will be handled by on workgroup. AMD recommends a wg size of 256, but
it may be better to bump it up on nvidia??
*/
layout (local_size_x = 16, local_size_y = 16, local_size_z = 1) in;

layout(binding = 0, rg32ui) uniform uimageBuffer visibility_buffer;

/* the position/size/damage of our windows */
layout(binding = 1) buffer tiles
{
	int width;
	int height;
	int active_tiles[];
};

struct Rect {
	vec2 start;
	vec2 end;
};

struct Window {
	int id;
	Rect dims;
	Rect opaque;
};

/* the position/size/damage of our windows */
layout(binding = 2) buffer window_list
{
	int window_count;
	Window windows[];
};

void main() {
	/* if this invocation extends past the resolution, then do nothing */
	if(gl_GlobalInvocationID.x >= width || gl_GlobalInvocationID.y >= height)
		return;

	/* TODO */
	ivec2 uv = ivec2(gl_GlobalInvocationID.xy);
}
