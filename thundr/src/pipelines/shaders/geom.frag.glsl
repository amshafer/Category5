#version 450
#extension GL_ARB_separate_shader_objects : enable
#extension GL_EXT_nonuniform_qualifier : enable

layout(location = 0) in vec2 coord;
layout(location = 0) out vec4 res;

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
 vec2 surface_offset;
} push;

/* The array of textures that are the window contents */
layout(set = 1, binding = 1) uniform sampler2D images[];

void main() {
 if (push.image_id >= 0) {
  res = texture(images[push.image_id], coord);
 }

 if (push.use_color > 0) {
  // If we have a color but also have an image, then
  // we should only update the color but keep the alpha
  // set by the image. This lets us color text for example.
  res = vec4(push.color.xyz,
             push.image_id >= 0 ? res.a : push.color.a);
 }
}
