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
float order;
float x;
float y;
float width;
float height;
} push;

void main() {
     // 1. loc should ALWAYS be 0,1 for the default quad.
     // 2. multiply by two since the axis are over the range (-1,1).
     // 3. multiply by the percentage of the screen that the window
     //    should take up. Any 1's in loc will be scaled by this amount.
     // 4. add the (x,y) offset for the window.
     // 5. also multiply the base by 2 for the same reason
     vec2 adjusted = loc
          * vec2(2, 2)
	  * (vec2(push.width, push.height) / vec2(ubo.width, ubo.height))
	  + (vec2(push.x, push.y) / vec2(ubo.width, ubo.height))
	  * vec2(2, 2);

     // The model transform will align x,y = (0, 0) with the top left of
     // the screen. It should stubtract 1.0 from x and y.
     gl_Position = ubo.model * vec4(adjusted, push.order, 1.0);

     fragcoord = coord;
}