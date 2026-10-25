// Spike S2 – brush tools: strokes captured as directions on the sphere, previewed on the GPU,
// committed to Rust for full-resolution rasterisation. The preview of a stroke disappears per tile
// as soon as the tile containing the committed result is resident (edit versions, see viewer.wgsl).

import { invoke } from "@tauri-apps/api/core";
import { Vec3 } from "./sphere";

export type Tool = "navigate" | "mask" | "clone";

export interface CommitResult {
  version: number;
  dirty: [number, number, number][];
  pixels: number;
  tiles_tested: number;
  ms_raster: number;
  ms_pyramid: number;
  ms_base: number;
  ms_total: number;
}

export interface Stroke {
  id: number;
  kind: "mask" | "clone";
  dirs: Vec3[];
  radiusDeg: number;
  hardness: number;
  opacity: number;
  rotation: [number, number, number, number] | null; // w, x, y, z
  /** edit version once committed; 0xffffffff while painting / committing */
  version: number;
  tDown: number;
  tUp?: number;
  tCommitted?: number;
  tVisible?: number;
  commit?: CommitResult;
  pending?: Set<number>;
  baseDone?: boolean;
  error?: string;
}

export const UNCOMMITTED = 0xffffffff;
const SEGS_PER_CHUNK = 8;
const DEG = Math.PI / 180;

const dot = (a: Vec3, b: Vec3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a: Vec3, b: Vec3): Vec3 => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const len = (a: Vec3) => Math.hypot(a[0], a[1], a[2]);
const norm = (a: Vec3): Vec3 => {
  const l = len(a);
  return [a[0] / l, a[1] / l, a[2] / l];
};
export const angle = (a: Vec3, b: Vec3) => Math.atan2(len(cross(a, b)), dot(a, b));

export function toYawPitch(d: Vec3): [number, number] {
  return [Math.atan2(d[0], -d[2]) / DEG, Math.asin(Math.max(-1, Math.min(1, d[1]))) / DEG];
}

export function fromYawPitch(yaw: number, pitch: number): Vec3 {
  const y = yaw * DEG, p = pitch * DEG;
  return [Math.sin(y) * Math.cos(p), Math.sin(p), -Math.cos(y) * Math.cos(p)];
}

/** Shortest rotation taking `a` to `b` as quaternion (w, x, y, z). */
export function rotationFromTo(a: Vec3, b: Vec3): [number, number, number, number] {
  const c = cross(a, b);
  const w = 1 + dot(a, b);
  const l = Math.hypot(w, c[0], c[1], c[2]);
  return [w / l, c[0] / l, c[1] / l, c[2] / l];
}

export function rotate(q: [number, number, number, number], v: Vec3): Vec3 {
  const u: Vec3 = [q[1], q[2], q[3]];
  const t0 = cross(u, v);
  const t: Vec3 = [2 * t0[0], 2 * t0[1], 2 * t0[2]];
  const c = cross(u, t);
  return [v[0] + q[0] * t[0] + c[0], v[1] + q[0] * t[1] + c[1], v[2] + q[0] * t[2] + c[2]];
}

function cap(points: Vec3[], extra: number): { c: Vec3; r: number } {
  const sum = points.reduce<Vec3>((s, p) => [s[0] + p[0], s[1] + p[1], s[2] + p[2]], [0, 0, 0]);
  const c = len(sum) > 1e-9 ? norm(sum) : points[0];
  return { c, r: Math.max(...points.map((p) => angle(c, p))) + extra };
}

export class Brushes {
  tool: Tool = "navigate";
  radiusDeg = 2;
  hardness = 0.6;
  opacity = 1;
  /** clone source (Alt-click) and the rotation derived on the first stroke after setting it */
  source: Vec3 | null = null;
  rotation: [number, number, number, number] | null = null;
  strokes: Stroke[] = [];
  active: Stroke | null = null;
  history: Stroke[] = [];
  private nextId = 1;
  dirty = true;

  begin(d: Vec3): Stroke | null {
    if (this.tool === "navigate") return null;
    if (this.tool === "clone") {
      if (!this.source) return null;
      if (!this.rotation) this.rotation = rotationFromTo(d, this.source); // aligned: keep the offset
    }
    const s: Stroke = {
      id: this.nextId++,
      kind: this.tool,
      dirs: [d],
      radiusDeg: this.radiusDeg,
      hardness: this.hardness,
      opacity: this.opacity,
      rotation: this.tool === "clone" ? this.rotation : null,
      version: UNCOMMITTED,
      tDown: performance.now(),
    };
    this.active = s;
    this.strokes.push(s);
    this.dirty = true;
    return s;
  }

  /** Adds a point if it is far enough from the last one (≥ radius/4, ≥ ~1 px). */
  extend(d: Vec3, pixelAngle: number, force = false): void {
    const s = this.active;
    if (!s) return;
    const last = s.dirs[s.dirs.length - 1];
    if (!force && angle(last, d) < Math.max((s.radiusDeg * DEG) / 4, pixelAngle)) return;
    s.dirs.push(d);
    this.dirty = true;
  }

  setSource(d: Vec3): void {
    this.source = d;
    this.rotation = null;
    this.dirty = true;
  }

  /** Ends the active stroke and commits it to Rust. */
  async end(session: number): Promise<Stroke | null> {
    const s = this.active;
    if (!s) return null;
    this.active = null;
    s.tUp = performance.now();
    try {
      const res = await invoke<CommitResult>("commit_stroke", {
        session,
        stroke: {
          kind: s.kind,
          points: s.dirs.map(toYawPitch),
          radius_deg: s.radiusDeg,
          hardness: s.hardness,
          opacity: s.opacity,
          rotation: s.rotation,
        },
      });
      s.commit = res;
      s.version = res.version;
      s.tCommitted = performance.now();
    } catch (e) {
      s.error = String(e);
      this.strokes = this.strokes.filter((x) => x !== s);
    }
    this.dirty = true;
    return s;
  }

  /** Called when a refreshed tile is resident / the base texture is updated. */
  settle(now: number): Stroke[] {
    const done: Stroke[] = [];
    for (const s of this.strokes) {
      if (s.version === UNCOMMITTED || !s.pending || !s.baseDone) continue;
      if (s.pending.size === 0) {
        s.tVisible = now;
        done.push(s);
      }
    }
    if (done.length) {
      this.strokes = this.strokes.filter((s) => !done.includes(s));
      this.history.push(...done);
      this.dirty = true;
    }
    return done;
  }

  /** GPU buffers: Stroke (64 B), Chunk (32 B), points (vec4f). */
  buffers(): { strokes: Uint8Array; chunks: Uint8Array; points: Float32Array; count: number } {
    const S = new ArrayBuffer(Math.max(1, this.strokes.length) * 64);
    const sf = new Float32Array(S), su = new Uint32Array(S);
    const chunks: number[][] = [];
    const pts: number[] = [];
    this.strokes.forEach((s, i) => {
      const r = s.radiusDeg * DEG;
      const dirs = s.dirs.length === 1 ? [s.dirs[0], s.dirs[0]] : s.dirs;
      const first = chunks.length;
      const p0 = pts.length / 4;
      for (const d of dirs) pts.push(d[0], d[1], d[2], 0);
      const nSeg = dirs.length - 1;
      for (let c = 0; c < nSeg; c += SEGS_PER_CHUNK) {
        const n = Math.min(SEGS_PER_CHUNK, nSeg - c);
        const k = cap(dirs.slice(c, c + n + 1), r);
        chunks.push([k.c[0], k.c[1], k.c[2], k.r >= Math.PI ? -2 : Math.cos(k.r), p0 + c, n]);
      }
      const all = cap(dirs, r);
      const q = s.rotation ?? [1, 0, 0, 0];
      sf.set([all.c[0], all.c[1], all.c[2], all.r >= Math.PI ? -2 : Math.cos(all.r)], i * 16);
      sf.set(q, i * 16 + 4);
      sf.set([r, r * Math.min(1, Math.max(0, s.hardness)), s.opacity, s.kind === "clone" ? 1 : 0], i * 16 + 8);
      su.set([first, chunks.length - first, s.version, 0], i * 16 + 12);
    });
    const C = new ArrayBuffer(Math.max(1, chunks.length) * 32);
    const cf = new Float32Array(C), cu = new Uint32Array(C);
    chunks.forEach((c, i) => {
      cf.set(c.slice(0, 4), i * 8);
      cu.set([c[4], c[5], 0, 0], i * 8 + 4);
    });
    // one spare point so `points[i + 1]` is always in range
    pts.push(0, 0, 0, 0);
    this.dirty = false;
    return { strokes: new Uint8Array(S), chunks: new Uint8Array(C), points: new Float32Array(pts), count: this.strokes.length };
  }
}
