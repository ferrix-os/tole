#version 450
// tole's GPU renderer: every drawing is one instance of a quad
// (docs/UI-FRAMEWORK.md §4.6). Compiled to quad.vert.spv by
// `glslangValidator -V --target-env vulkan1.3 quad.vert -o quad.vert.spv`;
// a host test checks the SPIR-V is this file's.

layout(push_constant) uniform Push {
    // The target's size in pixels.
    vec2 size;
} push;

// Where the quad goes, in the target's pixels: left, top, right, bottom.
layout(location = 0) in vec4 a_rect;
// The colour, premultiplied.
layout(location = 1) in vec4 a_color;
// The shape the quad covers: left, top, right, bottom.
layout(location = 2) in vec4 a_shape;
// radius, stroke width, kind, unused.
layout(location = 3) in vec4 a_params;
// Texture coordinates: a mask's texel at the quad's corner, or an
// image's normalised corners.
layout(location = 4) in vec4 a_uv;

layout(location = 0) flat out vec4 v_color;
layout(location = 1) flat out vec4 v_shape;
layout(location = 2) flat out vec4 v_params;
layout(location = 3) flat out vec4 v_uv;
layout(location = 4) flat out vec2 v_origin;

void main() {
    int i = gl_VertexIndex;
    vec2 corner = vec2(
        (i == 1 || i == 3 || i == 4) ? 1.0 : 0.0,
        (i == 2 || i == 4 || i == 5) ? 1.0 : 0.0);
    vec2 p = mix(a_rect.xy, a_rect.zw, corner);
    v_color = a_color;
    v_shape = a_shape;
    v_params = a_params;
    v_uv = a_uv;
    v_origin = a_rect.xy;
    gl_Position = vec4(p / push.size * 2.0 - 1.0, 0.0, 1.0);
}
