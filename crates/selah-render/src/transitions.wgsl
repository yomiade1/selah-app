// Frame/layer transition shader supporting Cut, Fade, Dissolve, FadeThroughBlack, Push, Wipe, Zoom, Slide.

struct TransitionParams {
    kind: u32,
    direction: u32,
    progress: f32,
    overlay_mode: u32,
};

@group(0) @binding(0) var<uniform> params: TransitionParams;
@group(0) @binding(1) var from_tex: texture_2d<f32>;
@group(0) @binding(2) var to_tex: texture_2d<f32>;
@group(0) @binding(3) var lin_sampler: sampler;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

var<private> BAYER: array<f32, 16> = array<f32, 16>(
    0.0, 8.0, 2.0, 10.0,
    12.0, 4.0, 14.0, 6.0,
    3.0, 11.0, 1.0, 9.0,
    15.0, 7.0, 13.0, 5.0
);

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    var out: VertexOut;
    out.position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

fn dir_axis(uv: vec2<f32>) -> vec3<f32> {
    // Returns (coord, to_starts_high (1.0=Left/Up, 0.0=Right/Down), is_vertical (0/1))
    switch params.direction {
        case 2u: { return vec3<f32>(uv.x, 0.0, 0.0); } // RIGHT: sweeps 0 -> 1
        case 3u: { return vec3<f32>(uv.y, 1.0, 1.0); } // UP: sweeps 1 -> 0
        case 4u: { return vec3<f32>(uv.y, 0.0, 1.0); } // DOWN: sweeps 0 -> 1
        default: { return vec3<f32>(uv.x, 1.0, 0.0); } // UNSPECIFIED / LEFT: sweeps 1 -> 0
    }
}

fn sample_shifted(tex: texture_2d<f32>, uv: vec2<f32>, delta: f32, is_vertical: f32) -> vec4<f32> {
    let offset = select(vec2<f32>(delta, 0.0), vec2<f32>(0.0, delta), is_vertical > 0.5);
    return textureSample(tex, lin_sampler, uv + offset);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let p = clamp(params.progress, 0.0, 1.0);
    let overlay = params.overlay_mode != 0u;
    let from_c = textureSample(from_tex, lin_sampler, in.uv);
    let to_c = textureSample(to_tex, lin_sampler, in.uv);

    // Cut (1) or Unspecified (0)
    if params.kind <= 1u {
        return select(from_c, to_c, (p > 0.0));
    }

    if p <= 0.0 {
        return from_c;
    }
    if p >= 1.0 {
        return to_c;
    }

    // Fade (2)
    if params.kind == 2u {
        return mix(from_c, to_c, p);
    }

    // Dissolve (3) via deterministic 4x4 Bayer matrix
    if params.kind == 3u {
        let ix = u32(floor(in.position.x)) & 3u;
        let iy = u32(floor(in.position.y)) & 3u;
        let threshold = (BAYER[iy * 4u + ix] + 0.5) / 16.0;
        return select(from_c, to_c, (p >= threshold));
    }

    // FadeThroughBlack (4)
    if params.kind == 4u {
        if p < 0.5 {
            let k = 1.0 - 2.0 * p;
            let out_a = select(from_c.a, from_c.a * k, overlay);
            return vec4<f32>(from_c.rgb * k, out_a);
        } else {
            let k = 2.0 * p - 1.0;
            let out_a = select(to_c.a, to_c.a * k, overlay);
            return vec4<f32>(to_c.rgb * k, out_a);
        }
    }

    let axis = dir_axis(in.uv);
    let coord = axis.x;
    let starts_high = axis.y > 0.5;
    let is_vert = axis.z;

    // Push (5)
    if params.kind == 5u {
        if starts_high {
            // Left / Up: boundary at 1 - p
            if coord >= 1.0 - p {
                return sample_shifted(to_tex, in.uv, -(1.0 - p), is_vert);
            } else {
                return sample_shifted(from_tex, in.uv, p, is_vert);
            }
        } else {
            // Right / Down: boundary at p
            if coord < p {
                return sample_shifted(to_tex, in.uv, 1.0 - p, is_vert);
            } else {
                return sample_shifted(from_tex, in.uv, -p, is_vert);
            }
        }
    }

    // Wipe (6)
    if params.kind == 6u {
        let signed_dist = select(p - coord, coord - (1.0 - p), starts_high);
        let w = smoothstep(-0.015, 0.015, signed_dist);
        return mix(from_c, to_c, w);
    }

    // Zoom (7)
    if params.kind == 7u {
        let scale = 0.5 + 0.5 * p;
        let zuv = (in.uv - vec2<f32>(0.5, 0.5)) / scale + vec2<f32>(0.5, 0.5);
        let inside = zuv.x >= 0.0 && zuv.x <= 1.0 && zuv.y >= 0.0 && zuv.y <= 1.0;
        let sampled_to = textureSample(to_tex, lin_sampler, clamp(zuv, vec2<f32>(0.0), vec2<f32>(1.0)));
        let fallback_to = select(vec4<f32>(0.0, 0.0, 0.0, 1.0), vec4<f32>(0.0), overlay);
        let zoom_to = select(fallback_to, sampled_to, inside);
        return mix(from_c, zoom_to, p);
    }

    // Slide (8): entering `to` slides over stationary `from`.
    if params.kind == 8u {
        let in_to = select((coord < p), (coord >= 1.0 - p), starts_high);
        let from_vis = select(from_c, from_c * (1.0 - p), overlay);
        if in_to {
            let delta = select(1.0 - p, -(1.0 - p), starts_high);
            let shifted_to = sample_shifted(to_tex, in.uv, delta, is_vert);
            return shifted_to + from_vis * (1.0 - shifted_to.a);
        }
        return from_vis;
    }

    return to_c;
}
