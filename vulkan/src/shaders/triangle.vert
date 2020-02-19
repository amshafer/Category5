#version 450
#extension GL_ARB_separate_shader_objects : enable

layout(location = 0) in vec2 loc;
layout(location = 1) in vec2 coord;

layout(location = 0) out vec2 fragcoord;

layout(binding = 0) uniform ShaderConstants {
mat4 model;
mat4 view;
mat4 proj;
} ubo;

void main() {
     gl_Position = ubo.model * vec4(loc, 0.5, 1.0);

     fragcoord = coord;
}