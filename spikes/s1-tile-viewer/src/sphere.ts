// Sphere maths shared by the tile scheduler. Conventions match viewer.wgsl and pyramid.rs:
// forward = -z, up = +y, u = 0.5 + atan2(x, -z)/2π, v = 0.5 - asin(y)/π.

export type Vec3 = [number, number, number];

export interface View {
  yaw: number; // radians
  pitch: number; // radians
  fov: number; // vertical field of view, radians
}

export interface Basis {
  f: Vec3;
  r: Vec3;
  u: Vec3;
}

export function basis(v: View): Basis {
  const cy = Math.cos(v.yaw), sy = Math.sin(v.yaw);
  const cp = Math.cos(v.pitch), sp = Math.sin(v.pitch);
  const f: Vec3 = [sy * cp, sp, -cy * cp];
  const r: Vec3 = [cy, 0, sy];
  const u: Vec3 = [-sy * sp, cp, cy * sp]; // r × f
  return { f, r, u };
}

/** Direction for normalised screen coords sx, sy ∈ [-1, 1] (sy up). */
export function rayDir(b: Basis, sx: number, sy: number, aspTan: number, tan: number): Vec3 {
  const x = b.f[0] + b.r[0] * sx * aspTan + b.u[0] * sy * tan;
  const y = b.f[1] + b.r[1] * sx * aspTan + b.u[1] * sy * tan;
  const z = b.f[2] + b.r[2] * sx * aspTan + b.u[2] * sy * tan;
  const l = Math.hypot(x, y, z);
  return [x / l, y / l, z / l];
}

export function dirToUv(d: Vec3): [number, number] {
  return [0.5 + Math.atan2(d[0], -d[2]) / (2 * Math.PI), 0.5 - Math.asin(Math.max(-1, Math.min(1, d[1]))) / Math.PI];
}

export function cubeFace(d: Vec3): number {
  const ax = Math.abs(d[0]), ay = Math.abs(d[1]), az = Math.abs(d[2]);
  if (ax >= ay && ax >= az) return d[0] > 0 ? 0 : 1;
  if (ay >= az) return d[1] > 0 ? 2 : 3;
  return d[2] > 0 ? 4 : 5;
}

export function cubeST(face: number, d: Vec3): [number, number] {
  const [x, y, z] = d;
  switch (face) {
    case 0: return [z / x, -y / x];
    case 1: return [-z / -x, -y / -x];
    case 2: return [x / y, z / y];
    case 3: return [x / -y, -z / -y];
    case 4: return [-x / z, -y / z];
    default: return [x / -z, -y / -z];
  }
}

export function angleBetween(a: Vec3, b: Vec3): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}
