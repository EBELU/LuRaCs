// Fill shader: fill the band between a data-space polyline and its baseline.
//
// A non-instanced draw of 6 * segments_per_pair * (segment count) vertices
// builds two triangles per sub-segment — a quad whose top edge is the
// interpolated curve and whose bottom edge is the baseline at the same x.
// `segments_per_pair` mirrors polyline.wgsl: 1 for linear, 2 for
// pre/post steps, 3 for mid-step. Drawn before the line so the stroke sits
// on top of its own fill (silx fill, §13 B1).

struct Params {
    ortho: mat4x4<f32>,
    color: vec4<f32>,        // premultiplied RGBA (target encoding)
    axis_log: vec2<f32>,     // 1.0 if that axis is log10, else 0.0
    interpolation: u32,      // 0=linear, 1=pre, 2=post, 3=mid
    // implicit 4-byte padding to 16-byte alignment
};

@group(0) @binding(0) var<storage, read> params: Params;
@group(0) @binding(1) var<storage, read> points: array<vec2<f32>>;
@group(0) @binding(2) var<storage, read> baseline: array<f32>;

const INV_LN10: f32 = 0.4342944819032518;

fn apply_scale(p: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(
        select(p.x, log(p.x) * INV_LN10, params.axis_log.x > 0.5),
        select(p.y, log(p.y) * INV_LN10, params.axis_log.y > 0.5),
    );
}

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> @builtin(position) vec4<f32> {
    var segments_per_pair = 1u;
    if (params.interpolation == 1u || params.interpolation == 2u) {
        segments_per_pair = 2u;
    } else if (params.interpolation == 3u) {
        segments_per_pair = 3u;
    }

    let seg = vid / (6u * segments_per_pair);
    let sub = (vid / 6u) % segments_per_pair;
    let corner = vid % 6u;

    // Per-corner endpoint selector (0 = sub-segment start, 1 = end) and
    // whether the corner is on the baseline (1) or the curve (0), for the
    // two triangles (curve0, base0, curve1) and (curve1, base0, base1).
    var endpoint = array<u32, 6>(0u, 0u, 1u, 1u, 0u, 1u);
    var is_base = array<u32, 6>(0u, 1u, 0u, 0u, 1u, 1u);

    let a = points[seg];
    let b = points[seg + 1u];

    // Sub-segment endpoints and the *original point index* each endpoint
    // derives from — same logic as polyline.wgsl, so the top edge of the
    // fill traces exactly the path the line strokes.
    var p0 = a;
    var p1 = b;
    var idx0 = seg;
    var idx1 = seg + 1u;

    if (params.interpolation == 1u) {          // pre
        if (sub == 0u) {
            p1 = vec2<f32>(a.x, b.y);
            idx1 = seg;
        } else {
            p0 = vec2<f32>(a.x, b.y);
        }
    } else if (params.interpolation == 2u) {   // post
        if (sub == 0u) {
            p1 = vec2<f32>(b.x, a.y);
        } else {
            p0 = vec2<f32>(b.x, a.y);
            idx0 = seg + 1u;
        }
    } else if (params.interpolation == 3u) {   // mid
        let mid_x = (a.x + b.x) * 0.5;
        if (sub == 0u) {
            p1 = vec2<f32>(mid_x, a.y);
            idx1 = seg;
        } else if (sub == 1u) {
            p0 = vec2<f32>(mid_x, a.y);
            p1 = vec2<f32>(mid_x, b.y);
        } else {
            p0 = vec2<f32>(mid_x, b.y);
            idx0 = seg + 1u;
        }
    }

    let ep = endpoint[corner];
    let pt = select(p0, p1, ep == 1u);
    // Baseline lookup uses the *original* index of the sub-segment endpoint
    // (matching the x position on the top edge, so the band is a rectangle
    // per sub-segment).
    let bi = min(select(idx0, idx1, ep == 1u), arrayLength(&baseline) - 1u);
    let yv = select(pt.y, baseline[bi], is_base[corner] == 1u);

    return params.ortho * vec4<f32>(apply_scale(vec2<f32>(pt.x, yv)), 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return params.color;
}