#version 450
#extension GL_ARB_separate_shader_objects : enable

layout(location = 0) in vec3 loc;
layout(location = 1) in vec3 norm;

layout(location = 0) out vec3 fragColor;

// this needs to be normalized
vec3 lightpos = vec3(-0.58, 0.58, -0.58);

layout(binding = 0) uniform ShaderConstants {
    mat4 model;
    mat4 view;
    mat4 proj;
} ubo;

void main() {
    gl_Position = ubo.proj * ubo.view * ubo.model * vec4(loc, 1.0);

    // adjust the give normal to reflect the model's transformation
    vec3 fragNorm = mat3(ubo.model) * norm;

    vec3 ambient = vec3(0.1, 0.1, 0.1);
    vec3 diffuse = vec3(0.5, 0.5, 0.5) * max(0.0, dot(fragNorm, lightpos));

    fragColor = diffuse + ambient;
}