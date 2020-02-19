#version 450
#extension GL_ARB_separate_shader_objects : enable

layout(location = 0) in vec2 coord;
layout(location = 0) out vec4 res;

layout(binding = 1) uniform sampler2D tex;

void main() {
    res = texture(tex, coord) + vec4(.01, .01, .01, 1.0);
}