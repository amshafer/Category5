#version 450
#extension GL_ARB_separate_shader_objects : enable

layout(location = 0) in vec2 coord;
layout(location = 0) out vec4 res;

layout(set = 1, binding = 1) uniform sampler2D tex;

layout(push_constant) uniform PushConstants {
vec4 color;
int  use_color;
float order;
float x;
float y;
float width;
float height;
} push;

void main() {
    if (push.use_color == 1) {
        res = push.color;
    } else {
        res = texture(tex, coord);
    }
}
