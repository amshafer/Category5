#version 450
#extension GL_ARB_separate_shader_objects : enable

layout(location = 0) in vec2 loc;
layout(location = 1) in vec2 coord;

layout(location = 0) out vec2 fragcoord;

layout(binding = 0) uniform ShaderConstants {
 mat4 model;
 float width;
 float height;
} ubo;

layout(push_constant) uniform PushConstants {
 float width;
 float height;
 // The id of the image. This is the offset into the unbounded sampler array.
 // id that's the offset into the unbound sampler array
 int image_id;
 // if we should use color instead of texturing
 int use_color;
 vec4 color;
 // The complete dimensions of the window.
 vec2 surface_pos;
 vec2 surface_size;
} push;

/* The array of textures that are the window contents */
layout(set = 1, binding = 1) uniform sampler2D images[];

void main() {
 // 1. loc should ALWAYS be 0,1 for the default quad.
 // 2. multiply by two since the axis are over the range (-1,1).
 // 3. multiply by the percentage of the screen that the window
 //    should take up. Any 1's in loc will be scaled by this amount.
 // 4. add the (x,y) offset for the window.
 // 5. also multiply the base by 2 for the same reason
 //
 // Use viewport size here instead of the total resolution size. We want
 // to scale around our display area, not the entire thing.
 vec2 adjusted = loc
  * vec2(2, 2)
  * (push.surface_size / vec2(push.width, push.height))
  + (push.surface_pos / vec2(push.width, push.height))
  * vec2(2, 2);

 gl_Position = ubo.model * vec4(adjusted, 0.0, 1.0);

 fragcoord = coord;
}
