// Tile scheduling: which tiles the current view needs, priority queue, transport, slot LRU,
// stale-request dropping and prefetch in the direction of movement.

import { fetchBytes, NotReady, Transport } from "./transport";
import { Renderer, Grid, Wire } from "./renderer";
import { angleBetween, basis, cubeFace, cubeST, dirToUv, rayDir, View } from "./sphere";

export interface LevelInfo { w: number; h: number; tiles_x: number; tiles_y: number }
export interface CubeLevelInfo { n: number; tiles: number }
export interface OpenInfo {
  session: number;
  path: string;
  kind: "jpeg" | "tiff" | "exr";
  width: number;
  height: number;
  storage: "u8" | "u16" | "f16";
  tile: number;
  slot: number;
  levels: LevelInfo[];
  cube_levels: CubeLevelInfo[];
  base_w: number;
  base_h: number;
  from_cache: boolean;
}

export type Layout = "equirect" | "cube";
export type { Transport };

export interface TileOptions {
  layout: Layout;
  wire: Wire;
  transport: Transport;
  slots: number;
  maxInflight: number;
  uploadsPerFrame: number;
  prefetch: boolean;
  aniso: boolean;
  bias: number;
}

const KEY_G = 1 << 20;
const KEY_Y = 1 << 10;
const key = (g: number, tx: number, ty: number) => g * KEY_G + ty * KEY_Y + tx;
/** [grid, tx, ty] */
const keyParts = (k: number) => [Math.floor(k / KEY_G), k % 1024, Math.floor(k / KEY_Y) % 1024];

export interface FrameStats {
  needed: number;
  missing: number;
  prefetch: number;
  inflight: number;
  resident: number;
  queued: number;
  centreLevel: number;
}

export class TileManager {
  readonly grids: Grid[] = [];
  readonly pages: Uint32Array;
  private slotKey: Int32Array;
  private slotUsed: Float64Array;
  readonly slotTime: Float32Array;
  private resident = new Map<number, number>();
  private inflight = new Map<number, AbortController>();
  private notReady = new Set<number>();
  private uploads: { k: number; buf: Uint8Array }[] = [];
  private needed = new Map<number, number>(); // key → priority (lower = sooner)
  private prefetchSet = new Set<number>();
  private neededSince = new Map<number, number>();
  private frame = 0;
  private pagesDirty = true;
  private lastViewKey = "";
  tilesReady = false;
  pxScale = 1;
  // metrics
  waits: number[] = [];
  fetchMs: number[] = [];
  serverMs: number[] = [];
  bytes = 0;
  fetched = 0;
  dropped = 0;
  evicted = 0;
  errors = 0;
  onError?: (msg: string) => void;
  /** Debug: paths of needed tiles that are not resident. */
  missingPaths(): string[] {
    return [...this.needed.keys()].filter((k) => !this.resident.has(k) && !this.prefetchSet.has(k))
      .map((k) => `${this.path(k)}${this.inflight.has(k) ? " (inflight)" : ""}${this.notReady.has(k) ? " (notReady)" : ""}`);
  }
  stats: FrameStats = { needed: 0, missing: 0, prefetch: 0, inflight: 0, resident: 0, queued: 0, centreLevel: -1 };

  constructor(private r: Renderer, readonly info: OpenInfo, readonly o: TileOptions) {
    let offset = 0;
    if (o.layout === "equirect") {
      for (const l of info.levels) {
        this.grids.push({ offset, tilesX: l.tiles_x, tilesY: l.tiles_y, w: l.w, h: l.h });
        offset += l.tiles_x * l.tiles_y;
      }
    } else {
      for (const l of info.cube_levels) {
        for (let f = 0; f < 6; f++) {
          this.grids.push({ offset, tilesX: l.tiles, tilesY: l.tiles, w: l.n, h: l.n });
          offset += l.tiles * l.tiles;
        }
      }
    }
    this.pages = new Uint32Array(offset);
    r.setup({
      slots: o.slots,
      slot: info.slot,
      wire: o.wire,
      grids: this.grids,
      pageCount: offset,
      levels: info.levels.length,
      level0H: info.levels[0].h,
      cube0N: info.cube_levels[0].n,
      baseH: info.base_h,
    });
    const n = r.slots;
    this.slotKey = new Int32Array(n).fill(-1);
    this.slotUsed = new Float64Array(n);
    this.slotTime = new Float32Array(n);
    r.writePages(this.pages);
  }

  dispose(): void {
    for (const c of this.inflight.values()) c.abort();
    this.inflight.clear();
  }

  private path(k: number): string {
    const [g, tx, ty] = keyParts(k);
    const s = this.info.session;
    if (this.o.layout === "equirect") return `${s}/e/${g}/${tx}/${ty}/${this.o.wire}`;
    return `${s}/c/${g % 6}/${Math.floor(g / 6)}/${tx}/${ty}/${this.o.wire}`;
  }

  /** Tiles needed for a view, sampled on a screen grid with the same LOD rule as the shader. */
  private collect(view: View, w: number, h: number, out: Map<number, number>, prioOffset: number): number {
    const b = basis(view);
    const tan = Math.tan(view.fov / 2);
    const aspTan = (w / h) * tan;
    // ~24 CSS px between samples, independent of the display's pixel ratio
    const step = Math.max(8, Math.round(24 * this.pxScale));
    const nLevels = this.info.levels.length;
    const H0 = this.info.levels[0].h;
    const N0 = this.info.cube_levels[0].n;
    const bias = this.o.bias;
    let centreLevel = -1;
    const add = (k: number, prio: number) => {
      const p = out.get(k);
      if (p === undefined || prio < p) out.set(k, prio);
    };
    for (let y = 0; y <= h + step - 1; y += step) {
      const py = Math.min(y, h - 1);
      for (let x = 0; x <= w + step - 1; x += step) {
        const px = Math.min(x, w - 1);
        const sx = ((px + 0.5) / w) * 2 - 1, sy = 1 - ((py + 0.5) / h) * 2;
        const d = rayDir(b, sx, sy, aspTan, tan);
        const dx = rayDir(b, sx + 2 / w, sy, aspTan, tan);
        const dy = rayDir(b, sx, sy - 2 / h, aspTan, tan);
        const a = Math.max(angleBetween(d, dx), angleBetween(d, dy));
        const prio = prioOffset + Math.hypot(px - w / 2, py - h / 2);
        const isCentre = Math.abs(px - w / 2) <= step / 2 && Math.abs(py - h / 2) <= step / 2;
        if (this.o.layout === "equirect") {
          const lf = Math.log2((a * H0) / Math.PI) + bias;
          const l = Math.floor(lf);
          if (isCentre) centreLevel = l;
          if (l >= nLevels) continue;
          const lv = Math.max(l, 0);
          const g = this.grids[lv];
          const [u, v] = dirToUv(d);
          // Each sample covers its whole grid cell (step px) so thin tiles between samples – e.g. the
          // partial last column at the seam, which shrinks to a wedge at the poles – are not missed.
          // Near the poles this also covers the u range of the shader's anisotropic taps.
          const T = this.info.tile;
          const cell = a * step;
          const cosLat = Math.max(Math.cos((v - 0.5) * Math.PI), 1e-4);
          const du = Math.min(1, (cell + (this.o.aniso ? a : 0)) / (2 * Math.PI * cosLat));
          const dv = cell / Math.PI;
          const y0 = Math.max(0, (v - dv / 2) * g.h), y1 = Math.min(g.h - 1, (v + dv / 2) * g.h);
          const x0 = (u - du / 2) * g.w, x1 = (u + du / 2) * g.w;
          const nx = Math.min(g.tilesX + 1, Math.ceil((x1 - x0) / T));
          for (let yy = y0; ; yy = Math.min(yy + T, y1)) {
            const ty = Math.min(Math.floor(yy / T), g.tilesY - 1);
            for (let i = 0; i <= nx; i++) {
              const x = Math.min(x0 + i * T, x1);
              const fx = ((x % g.w) + g.w) % g.w;
              add(key(lv, Math.min(Math.floor(fx / T), g.tilesX - 1), ty), prio);
            }
            if (yy >= y1) break;
          }
        } else {
          const f = cubeFace(d);
          const [s, t] = cubeST(f, d);
          const tpr = (0.5 * N0 * (1 + s * s + t * t)) / Math.sqrt(1 + Math.max(s * s, t * t));
          const l = Math.floor(Math.log2(a * tpr) + bias);
          if (isCentre) centreLevel = l;
          if (l >= nLevels) continue;
          const lv = Math.max(l, 0);
          const gi = lv * 6 + f;
          const g = this.grids[gi];
          // cover the sample's grid cell (texels per radian × cell angle)
          const half = (a * step * tpr) / 2;
          const T = this.info.tile;
          const cx = (s * 0.5 + 0.5) * g.w, cy = (t * 0.5 + 0.5) * g.h;
          const tx0 = Math.max(0, Math.floor((cx - half) / T)), tx1 = Math.min(g.tilesX - 1, Math.floor((cx + half) / T));
          const ty0 = Math.max(0, Math.floor((cy - half) / T)), ty1 = Math.min(g.tilesY - 1, Math.floor((cy + half) / T));
          for (let ty = ty0; ty <= ty1; ty++) for (let tx = tx0; tx <= tx1; tx++) add(key(gi, tx, ty), prio);
        }
      }
    }
    return centreLevel;
  }

  onTilesReady(): void {
    this.tilesReady = true;
    this.notReady.clear();
    this.lastViewKey = "";
  }

  /** Per frame: update needed set, start requests, upload finished tiles. Returns true if pages changed. */
  update(view: View, velocity: View, w: number, h: number, now: number, time: number): void {
    this.frame++;
    const vk = `${view.yaw.toFixed(5)},${view.pitch.toFixed(5)},${view.fov.toFixed(5)},${w},${h},${this.tilesReady}`;
    if (vk !== this.lastViewKey) {
      this.lastViewKey = vk;
      this.needed.clear();
      this.prefetchSet.clear();
      this.stats.centreLevel = this.collect(view, w, h, this.needed, 0);
      if (this.o.prefetch && (velocity.yaw || velocity.pitch || velocity.fov)) {
        const ahead = 0.35; // seconds
        const pv: View = {
          yaw: view.yaw + velocity.yaw * ahead,
          pitch: Math.max(-Math.PI / 2, Math.min(Math.PI / 2, view.pitch + velocity.pitch * ahead)),
          fov: Math.max(0.0005, view.fov + velocity.fov * ahead),
        };
        const pre = new Map<number, number>();
        this.collect(pv, w, h, pre, 1e6);
        for (const [k, p] of pre) {
          if (!this.needed.has(k)) {
            this.needed.set(k, p);
            this.prefetchSet.add(k);
          }
        }
      }
      // tiles that stopped being needed are no longer waited for
      for (const k of this.neededSince.keys()) {
        if (!this.needed.has(k) || this.prefetchSet.has(k)) this.neededSince.delete(k);
      }
    }

    let missing = 0, neededCount = 0;
    for (const k of this.needed.keys()) {
      const slot = this.resident.get(k);
      const isPre = this.prefetchSet.has(k);
      if (!isPre) neededCount++;
      if (slot !== undefined) {
        this.slotUsed[slot] = this.frame;
      } else if (!isPre && this.tilesReady) {
        missing++;
        if (!this.neededSince.has(k)) this.neededSince.set(k, now);
      }
    }

    // drop stale requests
    for (const [k, c] of this.inflight) {
      if (!this.needed.has(k)) {
        c.abort();
        this.inflight.delete(k);
        this.dropped++;
      }
    }

    // start new requests in priority order
    let queued = 0;
    if (this.tilesReady && this.inflight.size < this.o.maxInflight) {
      const want = [...this.needed]
        .filter(([k]) => !this.resident.has(k) && !this.inflight.has(k) && !this.notReady.has(k))
        .sort((a, b) => a[1] - b[1]);
      queued = want.length;
      for (const [k] of want) {
        if (this.inflight.size >= this.o.maxInflight) break;
        this.request(k);
      }
    }

    // uploads (budgeted per frame)
    let n = 0;
    while (this.uploads.length && n < this.o.uploadsPerFrame) {
      const up = this.uploads.shift()!;
      if (this.resident.has(up.k)) continue;
      const slot = this.allocSlot();
      if (slot < 0) {
        this.dropped++;
        continue;
      }
      this.r.writeTile(slot, up.buf);
      this.slotKey[slot] = up.k;
      this.slotUsed[slot] = this.frame;
      this.slotTime[slot] = time;
      this.resident.set(up.k, slot);
      const [g, tx, ty] = keyParts(up.k);
      this.pages[this.grids[g].offset + ty * this.grids[g].tilesX + tx] = slot + 1;
      this.pagesDirty = true;
      const since = this.neededSince.get(up.k);
      if (since !== undefined) {
        this.waits.push(now - since);
        this.neededSince.delete(up.k);
      }
      n++;
    }
    if (this.pagesDirty) {
      this.r.writePages(this.pages);
      this.r.writeSlotTimes(this.slotTime);
      this.pagesDirty = false;
    }
    this.stats = {
      ...this.stats,
      needed: neededCount,
      missing,
      prefetch: this.prefetchSet.size,
      inflight: this.inflight.size,
      resident: this.resident.size,
      queued,
    };
  }

  private allocSlot(): number {
    let best = -1, bestUsed = Infinity;
    for (let i = 0; i < this.slotKey.length; i++) {
      if (this.slotKey[i] < 0) return i;
      // never evict tiles needed in this frame
      if (this.slotUsed[i] < this.frame && this.slotUsed[i] < bestUsed) {
        best = i;
        bestUsed = this.slotUsed[i];
      }
    }
    if (best >= 0) {
      const old = this.slotKey[best];
      this.resident.delete(old);
      const [g, tx, ty] = keyParts(old);
      this.pages[this.grids[g].offset + ty * this.grids[g].tilesX + tx] = 0;
      this.evicted++;
    }
    return best;
  }

  private request(k: number): void {
    const ctrl = new AbortController();
    this.inflight.set(k, ctrl);
    const t0 = performance.now();
    fetchBytes(this.path(k), this.o.transport, ctrl.signal)
      .then(({ buf, serverMs }) => {
        if (this.inflight.get(k) !== ctrl) return;
        this.inflight.delete(k);
        this.fetchMs.push(performance.now() - t0);
        if (!isNaN(serverMs)) this.serverMs.push(serverMs);
        this.bytes += buf.byteLength;
        this.fetched++;
        this.uploads.push({ k, buf });
      })
      .catch((e) => {
        if (this.inflight.get(k) === ctrl) this.inflight.delete(k);
        if (e instanceof NotReady) this.notReady.add(k);
        else if (e?.name !== "AbortError") {
          this.notReady.add(k); // do not retry broken tiles every frame
          this.errors++;
          if (this.errors <= 5) this.onError?.(`tile ${this.path(k)} failed: ${e}`);
        }
      });
  }

  resetMetrics(): void {
    this.waits = [];
    this.fetchMs = [];
    this.serverMs = [];
    this.bytes = 0;
    this.fetched = 0;
    this.dropped = 0;
    this.evicted = 0;
  }
}

export function pct(a: number[], p: number): number {
  if (!a.length) return NaN;
  const s = [...a].sort((x, y) => x - y);
  return s[Math.min(s.length - 1, Math.floor((p / 100) * s.length))];
}
