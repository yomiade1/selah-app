// Test patterns (M1.5). Integer maths identical to cpu.rs `pattern_pixel`, written to an
// Rgba8Unorm target, so GPU and CPU output match exactly on every adapter.

struct Params {
    pattern: u32,   // 0 none, 1 SMPTE bars, 2 grid, 3 gradient
    cleared: u32,
    width: u32,
    height: u32,
};

@group(0) @binding(0) var<uniform> params: Params;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // One triangle that covers the whole target.
    let x = f32((index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(index & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn rgb(r: u32, g: u32, b: u32) -> vec4<f32> {
    return vec4<f32>(f32(r) / 255.0, f32(g) / 255.0, f32(b) / 255.0, 1.0);
}

fn bar_colour(bar: u32) -> vec4<f32> {
    switch bar {
        case 0u: { return rgb(191u, 191u, 191u); }
        case 1u: { return rgb(191u, 191u, 0u); }
        case 2u: { return rgb(0u, 191u, 191u); }
        case 3u: { return rgb(0u, 191u, 0u); }
        case 4u: { return rgb(191u, 0u, 191u); }
        case 5u: { return rgb(191u, 0u, 0u); }
        default: { return rgb(0u, 0u, 191u); }
    }
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let x = u32(floor(position.x));
    let y = u32(floor(position.y));
    let w = params.width;
    let h = params.height;
    if (params.cleared != 0u || params.pattern == 0u) {
        return rgb(0u, 0u, 0u);
    }
    if (params.pattern == 1u) {
        let bar = min(x * 7u / max(w, 1u), 6u);
        if (y < h * 2u / 3u) {
            return bar_colour(bar);
        }
        if (y < h * 3u / 4u) {
            if (bar % 2u == 1u) {
                return rgb(0u, 0u, 0u);
            }
            return bar_colour(6u - bar);
        }
        if (x < w / 4u) {
            return rgb(255u, 255u, 255u);
        }
        return rgb(16u, 16u, 16u);
    }
    if (params.pattern == 2u) {
        let step = max(h / 9u, 1u);
        let line = (x % step < 2u) || (y % step < 2u) || (x + 2u >= w) || (y + 2u >= h);
        if (line) {
            return rgb(255u, 255u, 255u);
        }
        return rgb(0u, 0u, 0u);
    }
    let v = x * 255u / max(w - 1u, 1u);
    return rgb(v, v, v);
}
