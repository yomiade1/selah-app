// Video layer (M1.7): NV12 planes → RGB, drawn under the text layer. The maths matches
// video.rs `yuv_to_rgb`, so the CPU reference agrees within rounding.

struct Params {
    out_width: u32,
    out_height: u32,
    video_width: u32,
    video_height: u32,
    matrix: u32,      // 0 BT.601, 1 BT.709
    full_range: u32,
    _pad0: u32,
    _pad1: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var luma: texture_2d<f32>;
@group(0) @binding(2) var chroma: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32((index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(index & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    // Nearest sample, integer mapping (scaling quality arrives with M3.4).
    let ox = u32(floor(position.x));
    let oy = u32(floor(position.y));
    let vx = min(ox * params.video_width / max(params.out_width, 1u), params.video_width - 1u);
    let vy = min(oy * params.video_height / max(params.out_height, 1u), params.video_height - 1u);
    let y8 = textureLoad(luma, vec2<i32>(i32(vx), i32(vy)), 0).r * 255.0;
    let uv8 = textureLoad(chroma, vec2<i32>(i32(vx / 2u), i32(vy / 2u)), 0).rg * 255.0;
    var y: f32;
    var cb: f32;
    var cr: f32;
    if (params.full_range != 0u) {
        y = y8 / 255.0;
        cb = (uv8.x - 128.0) / 255.0;
        cr = (uv8.y - 128.0) / 255.0;
    } else {
        y = (y8 - 16.0) / 219.0;
        cb = (uv8.x - 128.0) / 224.0;
        cr = (uv8.y - 128.0) / 224.0;
    }
    var rgb: vec3<f32>;
    if (params.matrix == 1u) {
        rgb = vec3<f32>(y + 1.5748 * cr, y - 0.187324 * cb - 0.468124 * cr, y + 1.8556 * cb);
    } else {
        rgb = vec3<f32>(y + 1.402 * cr, y - 0.344136 * cb - 0.714136 * cr, y + 1.772 * cb);
    }
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
