#version 450
#extension GL_ARB_separate_shader_objects : enable

/* Compute implementation of a compositor */

layout(binding = 0, rgba32f) uniform image2D frame;

layout(binding = 1) buffer windows
{
    int width;
    int height;
};

void main() {
    if(gl_GlobalInvocationID.x >= width || gl_GlobalInvocationID.y >= height)
        return;

    for (int i = 0; i < width; i++) {
        for (int j = 0; j < height; j++) {
            imageStore(frame, ivec2(j, i), vec4(0, 1, 0, 1));
        }
    }
}
