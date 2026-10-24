// Spike S1 – sphere viewer with virtual texturing.
// Every screen pixel computes its view direction, picks a pyramid level from its angular footprint,
// and walks from that level towards coarser ones until it finds resident tiles. Freshly uploaded
// tiles fade in over coarser data; the always-resident base texture is the last fallback.
// Two layouts share the code: equirect tiles (grid = level) and cube-face tiles (grid = level*6+face).

const MAX_GRIDS = 64u;

struct Uniforms {
  f: vec4f,          // camera forward (xyz)
  r: vec4f,          // camera right
  u: vec4f,          // camera up
  view: vec4f,       // asp*tan(fov/2), tan(fov/2), viewport width, height (device px)
  misc: vec4f,       // time (s), fade duration (s), lod bias, layout (0 equirect, 1 cube)
  misc2: vec4f,      // exposure (EV), linear source (0/1), aniso taps (0/1), debug (0 off, 1 levels)
  counts: vec4u,     // level count, slot size, tile size, base mip count
  level0: vec4f,     // equirect level-0 height, cube level-0 face size, base height, 0
  grids: array<vec4u, MAX_GRIDS>,     // page-table offset, tiles x, tiles y, 0
  gridDims: array<vec4f, MAX_GRIDS>,  // pixel width, height of that grid's image
}

@group(0) @binding(0) var<uniform> U: Uniforms;
@group(0) @binding(1) var<storage, read> pages: array<u32>;
@group(0) @binding(2) var<storage, read> slotTime: array<f32>;
@group(0) @binding(3) var atlas: texture_2d_array<f32>;
@group(0) @binding(4) var base: texture_2d<f32>;
@group(0) @binding(5) var linearClamp: sampler;
@group(0) @binding(6) var linearRepeat: sampler;

struct VOut { @builtin(position) pos: vec4f };

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VOut {
  var p = array<vec2f, 3>(vec2f(-1.0, -1.0), vec2f(3.0, -1.0), vec2f(-1.0, 3.0));
  var o: VOut;
  o.pos = vec4f(p[i], 0.0, 1.0);
  return o;
}

const PI = 3.14159265358979;
const TAU = 6.28318530717959;

fn dirToUv(d: vec3f) -> vec2f {
  return vec2f(0.5 + atan2(d.x, -d.z) / TAU, 0.5 - asin(clamp(d.y, -1.0, 1.0)) / PI);
}

// Cube face basis (normal, right, down) – must match pyramid.rs and tiles.ts.
fn cubeFace(d: vec3f) -> u32 {
  let a = abs(d);
  if (a.x >= a.y && a.x >= a.z) { return select(1u, 0u, d.x > 0.0); }
  if (a.y >= a.z) { return select(3u, 2u, d.y > 0.0); }
  return select(5u, 4u, d.z > 0.0);
}

fn cubeST(face: u32, d: vec3f) -> vec2f {
  switch face {
    case 0u: { return vec2f(d.z, -d.y) / d.x; }
    case 1u: { return vec2f(-d.z, -d.y) / -d.x; }
    case 2u: { return vec2f(d.x, d.z) / d.y; }
    case 3u: { return vec2f(d.x, -d.z) / -d.y; }
    case 4u: { return vec2f(-d.x, -d.y) / d.z; }
    default: { return vec2f(d.x, -d.y) / -d.z; }
  }
}

struct Hit { ok: bool, c: vec4f, fade: f32 }

fn tileSample(g: u32, p: vec2f) -> Hit {
  let gi = U.grids[g];
  let tile = f32(U.counts.z);
  let tx = min(u32(max(floor(p.x / tile), 0.0)), gi.y - 1u);
  let ty = min(u32(max(floor(p.y / tile), 0.0)), gi.z - 1u);
  let e = pages[gi.x + ty * gi.y + tx];
  if (e == 0u) { return Hit(false, vec4f(0.0), 0.0); }
  let slot = e - 1u;
  let local = p - vec2f(f32(tx), f32(ty)) * tile + 1.0; // +1 = gutter
  let uv = local / f32(U.counts.y);
  let c = textureSampleLevel(atlas, linearClamp, uv, i32(slot), 0.0);
  let fade = clamp((U.misc.x - slotTime[slot]) / U.misc.y, 0.0, 1.0);
  return Hit(true, c, fade);
}

// Debug tint per level: L0 red, L1 orange, L2 green, L3 blue, L4 violet, base grey.
fn levelTint(l: i32) -> vec3f {
  var t = array<vec3f, 6>(
    vec3f(1.0, 0.25, 0.25), vec3f(1.0, 0.75, 0.2), vec3f(0.3, 0.9, 0.3),
    vec3f(0.3, 0.6, 1.0), vec3f(0.8, 0.4, 1.0), vec3f(0.6, 0.6, 0.6));
  return t[clamp(l, 0, 5)];
}

// Walks from level `l0` to coarser levels, compositing fading tiles over what lies below them.
fn chain(l0: i32, uv: vec2f, cube: bool, face: u32, st: vec2f, baseC: vec4f) -> vec4f {
  var acc = vec4f(0.0);
  var rem = 1.0;
  let n = i32(U.counts.x);
  for (var l = max(l0, 0); l < n; l++) {
    var g = u32(l);
    var p: vec2f;
    if (cube) {
      g = u32(l) * 6u + face;
      p = (st * 0.5 + 0.5) * U.gridDims[g].xy;
    } else {
      p = vec2f(fract(uv.x), uv.y) * U.gridDims[g].xy;
    }
    let h = tileSample(g, p);
    if (h.ok) {
      var c = h.c;
      if (U.misc2.w > 0.5) { c = vec4f(mix(c.rgb, levelTint(l) * c.a, 0.35), c.a); }
      acc += rem * h.fade * c;
      rem *= 1.0 - h.fade;
      if (rem < 0.002) { return acc; }
    }
  }
  var b = baseC;
  if (U.misc2.w > 0.5) { b = vec4f(mix(b.rgb, levelTint(5) * b.a, 0.5), b.a); }
  return acc + rem * b;
}

fn srgbEncode(c: vec3f) -> vec3f {
  let lo = c * 12.92;
  let hi = 1.055 * pow(max(c, vec3f(0.0)), vec3f(1.0 / 2.4)) - 0.055;
  return select(hi, lo, c <= vec3f(0.0031308));
}

@fragment
fn fs(in: VOut) -> @location(0) vec4f {
  let sx = in.pos.x / U.view.z * 2.0 - 1.0;
  let sy = 1.0 - in.pos.y / U.view.w * 2.0;
  let d = normalize(U.f.xyz + U.r.xyz * sx * U.view.x + U.u.xyz * sy * U.view.y);
  // Angular size of this pixel (radians); derivatives of the direction are continuous everywhere.
  let a = max(length(dpdx(d)), length(dpdy(d)));
  let uv = dirToUv(d);
  let cube = U.misc.w > 0.5;
  let bias = U.misc.z;
  let nLevels = i32(U.counts.x);

  // Base texture: hardware mips, LOD from the vertical texel density (BASE_H/π texels per radian).
  let baseLod = max(log2(a * U.level0.z / PI) + bias, 0.0);
  let baseC = textureSampleLevel(base, linearRepeat, uv, baseLod);

  var c: vec4f;
  if (cube) {
    let face = cubeFace(d);
    let st = cubeST(face, d);
    // Minimum texel density over directions at (s,t): (N/2)(1+s²+t²)/sqrt(1+max(s²,t²)).
    let q = st * st;
    let tpr = 0.5 * U.level0.y * (1.0 + q.x + q.y) / sqrt(1.0 + max(q.x, q.y));
    let l = i32(floor(log2(a * tpr) + bias));
    c = chain(l, uv, true, face, st, baseC);
  } else {
    // Level from the vertical density H/π (sharp); horizontal oversampling 1/cos(lat) near the
    // poles is handled with extra taps along u.
    let fv = a * U.level0.x / PI;
    let lf = log2(fv) + bias;
    let l = i32(floor(lf));
    let cosLat = max(cos((uv.y - 0.5) * PI), 1e-4);
    var taps = 1u;
    if (U.misc2.z > 0.5 && l < nLevels) {
      let fu = exp2(lf - f32(max(l, 0))) / cosLat; // u footprint in texels at level l
      taps = u32(clamp(ceil(fu), 1.0, 8.0));
    }
    if (taps == 1u) {
      c = chain(l, uv, false, 0u, vec2f(0.0), baseC);
    } else {
      let du = a / (TAU * cosLat);
      c = vec4f(0.0);
      for (var k = 0u; k < taps; k++) {
        let o = ((f32(k) + 0.5) / f32(taps) - 0.5) * du;
        c += chain(l, uv + vec2f(o, 0.0), false, 0u, vec2f(0.0), baseC);
      }
      c /= f32(taps);
    }
  }

  var rgb = c.rgb;
  if (U.misc2.y > 0.5) {
    rgb = srgbEncode(rgb * exp2(U.misc2.x));
  }
  // Associated alpha (PTGui holes): composite over a checkerboard.
  let ck = select(0.32, 0.42, (u32(in.pos.x / 12.0) + u32(in.pos.y / 12.0)) % 2u == 0u);
  rgb = rgb + (1.0 - c.a) * vec3f(ck);
  return vec4f(rgb, 1.0);
}
