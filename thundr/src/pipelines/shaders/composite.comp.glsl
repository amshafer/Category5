#version 450
#extension GL_ARB_separate_shader_objects : enable

/* Compute implementation of a compositor */

/*
   Specify the size of our working groups to be 256

   We will tile the display into blocks of 256 pixels whose composition
   will be handled by on workgroup. AMD recommends a wg size of 256, but
   it may be better to bump it up on nvidia??
 */
layout (local_size_x = 16, local_size_y = 16, local_size_z = 1 ) in;

/* our render target: the swapchain frame to render into */
layout(binding = 0, rgba32f) uniform image2D frame;

layout(binding = 1) buffer tiles
{
	int width;
	int height;
	int active_tiles[];
};

layout(binding = 2, rg32ui) uniform uimageBuffer visibility_buffer;

/* The array of textures that are the window contents */
layout(binding = 3) uniform sampler2D images[];

void main() {
	/* if this invocation extends past the resolution, then do nothing */
	if(gl_GlobalInvocationID.x >= width || gl_GlobalInvocationID.y >= height)
		return;

	/* TODO */
	ivec2 uv = ivec2(gl_GlobalInvocationID.xy);

	imageStore(frame, uv, vec4(1, 1, 1, 1));
}
