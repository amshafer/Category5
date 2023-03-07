#version 450
#extension GL_ARB_separate_shader_objects : enable
#extension GL_EXT_nonuniform_qualifier : enable

layout(location = 0) in vec2 loc;
layout(location = 1) in vec2 coord;

layout(location = 0) out vec2 fragcoord;
layout(location = 1) flat out int window_index;
layout(location = 2) flat out int image_index;

layout(binding = 0) uniform ShaderConstants {
 mat4 model;
 float width;
 float height;
} ubo;

layout(push_constant) uniform PushConstants {
 vec2 viewport_offset;
 float width;
 float height;
 float starting_depth;
} push;

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

layout(set = 1, binding = 0, std140) buffer order_list
{
 layout(offset = 0) int window_count;
 /*
  * only vec4 types are tightly packed....
  *
  * So we have to extract ints from an array of ivec4s
  */
 layout(offset = 16) ivec4 ordered_windows[];
};

/* The array of textures that are the window contents */
layout(set = 2, binding = 1) uniform sampler2D images[];

void main() {
 int vec_offset = gl_InstanceIndex / 4;
 window_index = ordered_windows[vec_offset][gl_InstanceIndex % 4];
 image_index = windows[window_index].image_id;

 // Add our viewport offset to the location
 vec2 position = windows[window_index].dims.start + push.viewport_offset;

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
  * (windows[window_index].dims.size / vec2(push.width, push.height))
  + (position / vec2(push.width, push.height))
  * vec2(2, 2);

 // use our instance number as the depth. Smaller means farther back in
 // the scene, so we are drawing back to front but our depth value is
 // increasing
 //
 // We also have a starting depth that is set, which keeps track of the
 // latest depth to start at. This will be updated every time a surface
 // list is drawn in thundr, that way lists don't Z-fight
 // One million objects is our max right now.
 float order = push.starting_depth + float(gl_InstanceIndex) / 1000000000.0;
 gl_Position = ubo.model * vec4(adjusted, order, 1.0);

 fragcoord = coord;
}
