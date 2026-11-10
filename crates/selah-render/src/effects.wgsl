// Closed built-in GPU effect shader (M3.5 / SEC-03 / SEC-04).
// Stages: 2 = ColourAdjust, 3 = Vignette, 4 = BackgroundBlur composite, 5 = ColourKey, 6 = Mask.

struct EffectParams {
    kind: u32,
    is_output: u32,
    flags: u32,
    mix_amount: f32,
    p0: vec4<f32>,
    p1: vec4<f32>,
    p2: vec4<f32>,
};

@group(0) @binding(0) var<uniform> params: EffectParams;
@group(0) @binding(1) var primary_tex: texture_2d<f32>;
@group(0) @binding(2) var secondary_tex: texture_2d<f32>;
@group(0) @binding(3) var tertiary_tex: texture_2d<f32>;
@group(0) @binding(4) var tex_sampler: sampler;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    var out: VertexOut;
    out.position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let c = textureSample(primary_tex, tex_sampler, in.uv);

    // 2: ColourAdjust
    if params.kind == 2u {
        let a = c.a;
        let rgb0 = select(
            vec3<f32>(0.0),
            clamp(c.rgb / max(a, 1e-5), vec3<f32>(0.0), vec3<f32>(1.0)),
            a > 1e-5
        );
        var rgb = rgb0 + vec3<f32>(params.p0.x);
        rgb = (rgb - vec3<f32>(0.5)) * params.p0.y + vec3<f32>(0.5);
        rgb = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));
        let luma = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        rgb = mix(vec3<f32>(luma), rgb, params.p0.z);
        let theta = params.p0.w;
        if abs(theta) > 1e-5 {
            let cos_h = cos(theta);
            let sin_h = sin(theta);
            let k = vec3<f32>(0.57735027, 0.57735027, 0.57735027);
            rgb = rgb * cos_h + cross(k, rgb) * sin_h + k * dot(k, rgb) * (1.0 - cos_h);
        }
        rgb = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));
        let out_rgb = mix(rgb0, rgb, params.mix_amount);
        return vec4<f32>(out_rgb * a, a);
    }

    // 3: Vignette
    if params.kind == 3u {
        let r = length((in.uv - vec2<f32>(0.5, 0.5)) * 2.0);
        let inner = params.p0.x;
        let softness = max(params.p0.y, 1e-4);
        let strength = clamp(params.p0.z * params.mix_amount, 0.0, 1.0);
        let edge = smoothstep(inner, inner + softness, r);
        let keep = 1.0 - edge * strength;
        let vig_rgb = params.p1.rgb * c.a;
        return vec4<f32>(mix(vig_rgb, c.rgb, keep), c.a);
    }

    // 4: BackgroundBlur composite (primary=backdrop, secondary=blurred_backdrop, tertiary=layer)
    if params.kind == 4u {
        let blurred = textureSample(secondary_tex, tex_sampler, in.uv);
        let layer = textureSample(tertiary_tex, tex_sampler, in.uv);
        let has_region = (params.flags & 1u) != 0u;
        var mask = 0.0;
        if has_region {
            let rx = params.p0.x;
            let ry = params.p0.y;
            let rw = params.p0.z;
            let rh = params.p0.w;
            let in_box = in.uv.x >= rx && in.uv.x <= rx + rw && in.uv.y >= ry && in.uv.y <= ry + rh;
            mask = select(0.0, params.mix_amount, in_box);
        } else {
            mask = select(0.0, params.mix_amount, layer.a > 0.002);
        }
        let tint_a = clamp(params.p1.a, 0.0, 1.0);
        let tinted = vec4<f32>(
            params.p1.rgb * tint_a + blurred.rgb * (1.0 - tint_a),
            max(blurred.a, tint_a)
        );
        return mix(c, tinted, mask);
    }

    // 5: ColourKey
    if params.kind == 5u {
        let a = c.a;
        let rgb = select(
            vec3<f32>(0.0),
            clamp(c.rgb / max(a, 1e-5), vec3<f32>(0.0), vec3<f32>(1.0)),
            a > 1e-5
        );
        let dist = length(rgb - params.p0.rgb) / 1.7320508;
        let tol = params.p0.w;
        let soft = max(params.p1.x, 1e-4);
        let keep = smoothstep(tol, tol + soft, dist);
        let spill = clamp(params.p1.y, 0.0, 1.0);
        let luma = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        let suppressed_rgb = mix(rgb, vec3<f32>(luma), (1.0 - keep) * spill);
        let factor = mix(1.0, keep, params.mix_amount);
        if params.is_output != 0u {
            return vec4<f32>(suppressed_rgb * a * factor, a);
        }
        return vec4<f32>(suppressed_rgb * (a * factor), a * factor);
    }

    // 6: Mask
    if params.kind == 6u {
        let shape = (params.flags >> 1u) & 3u;
        let invert = (params.flags & 1u) != 0u;
        let b = params.p0;
        let center = vec2<f32>(b.x + b.z * 0.5, b.y + b.w * 0.5);
        let he = max(vec2<f32>(b.z * 0.5, b.w * 0.5), vec2<f32>(1e-4, 1e-4));
        let corner = clamp(params.p1.x, 0.0, min(he.x, he.y));
        let feather = max(params.p1.y, 1e-4);
        let aspect = max(params.p1.z, 1e-4);

        var dist = 0.0;
        if shape == 2u {
            let d = vec2<f32>((in.uv.x - center.x) * aspect, in.uv.y - center.y);
            let rad = min(he.x * aspect, he.y);
            dist = length(d) - rad;
        } else if shape == 3u {
            let p = (in.uv - center) / he;
            dist = (length(p) - 1.0) * min(he.x, he.y);
        } else {
            let q = abs(in.uv - center) - (he - vec2<f32>(corner, corner));
            dist = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - corner;
        }
        var inside = 1.0 - smoothstep(-feather * 0.5, feather * 0.5, dist);
        if invert {
            inside = 1.0 - inside;
        }
        let factor = mix(1.0, inside, params.mix_amount);
        if params.is_output != 0u {
            return vec4<f32>(c.rgb * factor, c.a);
        }
        return c * factor;
    }

    return c;
}
