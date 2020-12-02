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

/* the position/size/damage of our windows */
layout(binding = 1) buffer windows
{
    int width;
    int height;
};

void main() {
    /* if this invocation extends past the resolution, then do nothing */
    if(gl_GlobalInvocationID.x >= width || gl_GlobalInvocationID.y >= height)
        return;

    /* get the position of this invocation in the framebuffer */
    uint x = gl_GlobalInvocationID.x / width;
    uint y = gl_GlobalInvocationID.y / height;

    imageStore(frame, ivec2(x, y), vec4(0, 1, 0, 1));
}
