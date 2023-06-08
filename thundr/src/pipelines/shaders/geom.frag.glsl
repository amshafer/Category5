#version 450
#extension GL_ARB_separate_shader_objects : enable
#extension GL_EXT_nonuniform_qualifier : enable

layout(location = 0) in vec2 coord;
layout(location = 1) flat in int window_index;
layout(location = 2) flat in int image_index;
layout(location = 0) out vec4 res;

struct Rect {
 ivec2 start;
 ivec2 size;
};

struct Window {
 /* id that's the offset into the unbound sampler array */
 int image_id;
 /* if we should use w_color instead of texturing */
 int use_color;
 /* for alignment purposes */
 int pad_1;
 int pad_2;
 /* the color used instead of texturing */
 vec4 color;
 Rect dims;
 Rect opaque;
};

/* the position/size/damage of our windows */
layout(set = 2, binding = 0, std140) buffer window_list
{
 layout(offset = 0) int total_window_count;
 layout(offset = 16) Window windows[];
};

layout(set = 1, binding = 1, std140) buffer order_list
{
 layout(offset = 0) int window_count;
 layout(offset = 16) int ordered_windows[];
};

/* The array of textures that are the window contents */
layout(set = 2, binding = 1) uniform sampler2D images[];

void main() {
 if (windows[window_index].image_id >= 0) {
  res = texture(images[image_index], coord);
 }

 if (windows[window_index].use_color > 0) {
  // If we have a color but also have an image, then
  // we should only update the color but keep the alpha
  // set by the image. This lets us color text for example.
  res = vec4(windows[window_index].color.xyz,
             windows[window_index].image_id >= 0
              ? res.a : windows[window_index].color.a);
 }
}
