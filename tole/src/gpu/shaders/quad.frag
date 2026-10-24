#version 450
// tole's GPU renderer's one fragment shader: what an instance's kind
// draws (docs/UI-FRAMEWORK.md §4.6). Compiled to quad.frag.spv by
// `glslangValidator -V --target-env vulkan1.3 quad.frag -o quad.frag.spv`;
// a host test checks the SPIR-V is this file's.
//
// Kinds: 0 a solid rectangle, its pixels those whose centres it holds;
// 1 a rounded rectangle filled, antialiased; 2 a rectangle's outline
// (a rounded one's too) stroked along the shape's edge; 3 a coverage mask
// from the atlas in the colour (glyphs, paths); 4 a picture scaled into
// the shape, at the colour's alpha; 5 a layer, texel for pixel, at the
// colour's alpha.

layout(set = 0, binding = 0) uniform sampler2D tex;

layout(location = 0) flat in vec4 v_color;
layout(location = 1) flat in vec4 v_shape;
layout(location = 2) flat in vec4 v_params;
layout(location = 3) flat in vec4 v_uv;
layout(location = 4) flat in vec2 v_origin;

layout(location = 0) out vec4 o_color;

// Distance from a rounded box of half-size `half` and corner radius `r`,
// centred on the origin; a square-cornered box measures by the larger
// axis, so its outline's outer corners stay square, as a mitred join is.
float rounded_box(vec2 p, vec2 half_size, float r) {
    if (r <= 0.0) {
        vec2 d = abs(p) - half_size;
        return max(d.x, d.y);
    }
    vec2 q = abs(p) - half_size + vec2(r);
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

void main() {
    int kind = int(v_params.z + 0.5);
    vec2 p = gl_FragCoord.xy;
    if (kind == 0) {
        o_color = v_color;
    } else if (kind == 1 || kind == 2) {
        vec2 centre = (v_shape.xy + v_shape.zw) * 0.5;
        vec2 half_size = (v_shape.zw - v_shape.xy) * 0.5;
        float d = rounded_box(p - centre, half_size, v_params.x);
        if (kind == 2) {
            d = abs(d) - v_params.y * 0.5;
        }
        o_color = v_color * clamp(0.5 - d, 0.0, 1.0);
    } else if (kind == 3) {
        ivec2 texel = ivec2(floor(p - v_origin)) + ivec2(v_uv.xy);
        o_color = v_color * texelFetch(tex, texel, 0).r;
    } else if (kind == 4) {
        vec2 t = (p - v_shape.xy) / max(v_shape.zw - v_shape.xy, vec2(1e-6));
        o_color = texture(tex, mix(v_uv.xy, v_uv.zw, t)) * v_color.a;
    } else {
        o_color = texelFetch(tex, ivec2(floor(p)), 0) * v_color.a;
    }
}
