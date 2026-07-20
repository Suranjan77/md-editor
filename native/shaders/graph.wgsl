// Knowledge-graph GPU renderer.
//
// Two instanced draws (edges, then nodes) composite over Iced's existing frame
// with straight alpha blending. All geometry is generated in the vertex shader
// from a 6-vertex quad, so the only per-instance data are the vertex buffers
// bound in `gpu_graph.rs`; there are no storage buffers (which is what let the
// old version trip `VERTEX_WRITABLE_STORAGE` and crash on pipeline creation).

struct Globals {
    // world -> clip: clip.x = t.x * world.x + t.z, clip.y = t.y * world.y + t.w
    transform: vec4<f32>,
    // logical widget size in points, used to size pixel-space radii/thickness
    viewport: vec2<f32>,
    // 1.0 when the render target is an *_Srgb format (output must be linear)
    srgb: f32,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> globals: Globals;

// Colors arrive as non-linear sRGB (matching iced::Color). An sRGB target
// re-encodes on store and blends in linear, so convert before output there.
fn encode(rgb: vec3<f32>) -> vec3<f32> {
    if (globals.srgb < 0.5) {
        return rgb;
    }
    let cutoff = step(vec3<f32>(0.04045), rgb);
    let low = rgb / 12.92;
    let high = pow((rgb + 0.055) / 1.055, vec3<f32>(2.4));
    return mix(low, high, cutoff);
}

fn world_to_clip(p: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(
        globals.transform.x * p.x + globals.transform.z,
        globals.transform.y * p.y + globals.transform.w,
    );
}

// A pixel-space offset expressed in clip space (clip spans 2 units per axis).
fn px_to_clip(px: vec2<f32>) -> vec2<f32> {
    return px * vec2<f32>(2.0 / globals.viewport.x, 2.0 / globals.viewport.y);
}

fn quad_corner(vi: u32) -> vec2<f32> {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>( 1.0,  1.0),
    );
    return corners[vi];
}

// ─────────────────────────────── edges ───────────────────────────────

struct EdgeIn {
    @location(0) p0: vec2<f32>,
    @location(1) p1: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) thickness: f32,
};

struct EdgeOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) across: f32,
};

@vertex
fn vs_edge(in: EdgeIn, @builtin(vertex_index) vi: u32) -> EdgeOut {
    let c0 = world_to_clip(in.p0);
    let c1 = world_to_clip(in.p1);

    // Direction/normal computed in pixel space so thickness stays uniform
    // regardless of the widget aspect ratio.
    let d_px = (c1 - c0) * globals.viewport * 0.5;
    let len = max(length(d_px), 0.0001);
    let dir = d_px / len;
    let normal = vec2<f32>(-dir.y, dir.x);
    let half_off = px_to_clip(normal * (in.thickness * 0.5));

    let corner = quad_corner(vi);
    let base = select(c0, c1, corner.x > 0.0);

    var out: EdgeOut;
    out.pos = vec4<f32>(base + half_off * corner.y, 0.0, 1.0);
    out.color = in.color;
    out.across = corner.y;
    return out;
}

@fragment
fn fs_edge(in: EdgeOut) -> @location(0) vec4<f32> {
    let aa = fwidth(in.across) * 1.5;
    let edge = 1.0 - smoothstep(1.0 - aa, 1.0, abs(in.across));
    return vec4<f32>(encode(in.color.rgb), in.color.a * edge);
}

// ─────────────────────────────── nodes ───────────────────────────────

struct NodeIn {
    @location(0) center: vec2<f32>,
    @location(1) radius: f32,   // core radius, in points
    @location(2) glow: f32,     // outer-halo strength, 0..1
    @location(3) color: vec4<f32>,
    @location(4) ring: f32,     // 0 none, 1 hover (white), 2 selected (gold)
    @location(5) dim: f32,      // alpha multiplier for focus dimming
};

struct NodeOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,          // -1..1 across the padded quad
    @location(2) params: vec4<f32>,      // core_frac, glow, ring, dim
};

// Quad is padded well past the core so the halo has room to fade out.
const EXTENT_MUL: f32 = 2.9;

@vertex
fn vs_node(in: NodeIn, @builtin(vertex_index) vi: u32) -> NodeOut {
    let extent = max(in.radius, 0.5) * EXTENT_MUL;
    let center = world_to_clip(in.center);
    let corner = quad_corner(vi);

    var out: NodeOut;
    out.pos = vec4<f32>(center + px_to_clip(corner * extent), 0.0, 1.0);
    out.uv = corner;
    out.color = in.color;
    out.params = vec4<f32>(in.radius / extent, in.glow, in.ring, in.dim);
    return out;
}

@fragment
fn fs_node(in: NodeOut) -> @location(0) vec4<f32> {
    let core_frac = in.params.x;
    let glow = in.params.y;
    let ring = in.params.z;
    let dim = in.params.w;

    let d = length(in.uv);           // 0 at center, 1 at quad edge
    let aa = fwidth(d) * 1.5;

    // Solid, anti-aliased core disc.
    let core = 1.0 - smoothstep(core_frac - aa, core_frac + aa, d);

    // Soft luminous halo falling off toward the quad edge.
    let halo = glow * pow(clamp(1.0 - d, 0.0, 1.0), 2.5);

    var rgb = in.color.rgb;
    var alpha = max(core, halo);

    // Selection / hover ring: a bright annulus hugging the core edge.
    if (ring > 0.5) {
        let center_r = core_frac + core_frac * 0.34;
        let width = core_frac * 0.30 + aa;
        let ring_a = 1.0 - smoothstep(width, width + aa, abs(d - center_r));
        if (ring > 1.5) {
            rgb = mix(rgb, vec3<f32>(1.0, 0.83, 0.35), ring_a);
            alpha = max(alpha, ring_a);
        } else {
            rgb = mix(rgb, vec3<f32>(1.0, 1.0, 1.0), ring_a * 0.85);
            alpha = max(alpha, ring_a * 0.9);
        }
    }

    // Brighten the very center for a bit of dimensionality.
    rgb = rgb + (1.0 - smoothstep(0.0, core_frac, d)) * 0.18;

    alpha = alpha * dim * in.color.a;
    if (alpha < 0.003) {
        discard;
    }
    return vec4<f32>(encode(rgb), alpha);
}
