// WebGPU resources and the full-screen sphere pass.

import shaderCode from "./viewer.wgsl?raw";
import { basis, View } from "./sphere";

export type Wire = "u8" | "f16";

export interface Grid {
  offset: number;
  tilesX: number;
  tilesY: number;
  w: number;
  h: number;
}

export interface RenderParams {
  view: View;
  time: number;
  fade: number;
  bias: number;
  cube: boolean;
  exposure: number;
  linear: boolean;
  aniso: boolean;
  debug: boolean;
}

const MAX_GRIDS = 64;
const GRID_OFFSET = 44; // words before the grid arrays
const UNIFORM_WORDS = GRID_OFFSET + MAX_GRIDS * 4 * 2;

/** S2: brush cursor / stroke preview state passed to the shader. */
export interface BrushParams {
  cursor: [number, number, number] | null;
  radius: number; // radians
  source: [number, number, number] | null;
  strokeCount: number;
  baseVersion: number;
}

export class Renderer {
  device!: GPUDevice;
  adapterInfo = "";
  maxLayers = 256;
  features = "";
  /** Last measured GPU time of the sphere pass in ms (timestamp-query), NaN if unavailable. */
  gpuMs = NaN;
  private querySet?: GPUQuerySet;
  private queryResolve?: GPUBuffer;
  private queryRead?: GPUBuffer;
  private queryBusy = false;
  private ctx!: GPUCanvasContext;
  private format!: GPUTextureFormat;
  private pipeline!: GPURenderPipeline;
  private uniform!: GPUBuffer;
  private u = new ArrayBuffer(UNIFORM_WORDS * 4);
  private uf = new Float32Array(this.u);
  private uu = new Uint32Array(this.u);
  private atlas?: GPUTexture;
  private base?: GPUTexture;
  private pages?: GPUBuffer;
  private slotTime?: GPUBuffer;
  private bindGroup?: GPUBindGroup;
  private strokeBuf?: GPUBuffer;
  private chunkBuf?: GPUBuffer;
  private pointBuf?: GPUBuffer;
  private slotVersion?: GPUBuffer;
  private samplerClamp!: GPUSampler;
  private samplerRepeat!: GPUSampler;
  slot = 514;
  wire: Wire = "u8";
  slots = 0;

  constructor(private canvas: HTMLCanvasElement) {}

  async init(): Promise<void> {
    if (!("gpu" in navigator)) throw new Error("navigator.gpu missing – WebGPU not available in this webview");
    const adapter = await navigator.gpu.requestAdapter({ powerPreference: "high-performance" });
    if (!adapter) throw new Error("no WebGPU adapter");
    const info = adapter.info;
    this.adapterInfo = [info?.vendor, info?.architecture, info?.device, info?.description].filter(Boolean).join(" ");
    this.maxLayers = adapter.limits.maxTextureArrayLayers;
    this.features = [...adapter.features].sort().join(" ");
    const ts = adapter.features.has("timestamp-query");
    this.device = await adapter.requestDevice({
      requiredLimits: { maxTextureArrayLayers: this.maxLayers },
      requiredFeatures: ts ? ["timestamp-query"] : [],
    });
    if (ts) {
      this.querySet = this.device.createQuerySet({ type: "timestamp", count: 2 });
      this.queryResolve = this.device.createBuffer({ size: 16, usage: GPUBufferUsage.QUERY_RESOLVE | GPUBufferUsage.COPY_SRC });
      this.queryRead = this.device.createBuffer({ size: 16, usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST });
    }
    this.device.lost.then((l) => console.error("device lost", l.message));
    this.ctx = this.canvas.getContext("webgpu")!;
    this.format = navigator.gpu.getPreferredCanvasFormat();
    this.ctx.configure({ device: this.device, format: this.format, alphaMode: "opaque" });
    const module = this.device.createShaderModule({ code: shaderCode });
    const ci = await module.getCompilationInfo();
    for (const m of ci.messages) console[m.type === "error" ? "error" : "warn"]("WGSL", m.lineNum, m.message);
    this.pipeline = this.device.createRenderPipeline({
      layout: "auto",
      vertex: { module, entryPoint: "vs" },
      fragment: { module, entryPoint: "fs", targets: [{ format: this.format }] },
      primitive: { topology: "triangle-list" },
    });
    this.uniform = this.device.createBuffer({
      size: UNIFORM_WORDS * 4,
      usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
    });
    this.samplerClamp = this.device.createSampler({ magFilter: "linear", minFilter: "linear" });
    this.samplerRepeat = this.device.createSampler({
      magFilter: "linear",
      minFilter: "linear",
      mipmapFilter: "linear",
      addressModeU: "repeat",
    });
  }

  /** (Re)creates atlas, page table and slot-time buffers. */
  setup(opts: { slots: number; slot: number; wire: Wire; grids: Grid[]; pageCount: number; levels: number;
    level0H: number; cube0N: number; baseH: number }): void {
    this.atlas?.destroy();
    this.pages?.destroy();
    this.slotTime?.destroy();
    this.slots = Math.min(opts.slots, this.maxLayers);
    this.slot = opts.slot;
    this.wire = opts.wire;
    this.atlas = this.device.createTexture({
      size: [opts.slot, opts.slot, this.slots],
      format: opts.wire === "u8" ? "rgba8unorm" : "rgba16float",
      usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST,
    });
    this.pages = this.device.createBuffer({
      size: Math.max(16, opts.pageCount * 4),
      usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST,
    });
    this.slotTime = this.device.createBuffer({
      size: Math.max(16, this.slots * 4),
      usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST,
    });
    this.slotVersion?.destroy();
    this.slotVersion = this.device.createBuffer({
      size: Math.max(16, this.slots * 4),
      usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST,
    });
    if (!this.strokeBuf) this.writeStrokes(new Uint8Array(64), new Uint8Array(32), new Float32Array(8));
    if (opts.grids.length > MAX_GRIDS) throw new Error("too many grids");
    opts.grids.forEach((g, i) => {
      this.uu.set([g.offset, g.tilesX, g.tilesY, 0], GRID_OFFSET + i * 4);
      this.uf.set([g.w, g.h, 0, 0], GRID_OFFSET + MAX_GRIDS * 4 + i * 4);
    });
    this.uu.set([opts.levels, opts.slot, opts.slot - 2, 0], 24);
    this.uf.set([opts.level0H, opts.cube0N, opts.baseH, 0], 28);
    if (!this.base) this.setBase(null, 1, 1, 1, "u8");
    this.rebind();
  }

  setBase(data: Uint8Array | null, w: number, h: number, mips: number, wire: Wire): void {
    this.base?.destroy();
    const bpp = wire === "u8" ? 4 : 8;
    this.base = this.device.createTexture({
      size: [w, h],
      mipLevelCount: mips,
      format: wire === "u8" ? "rgba8unorm" : "rgba16float",
      usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST,
    });
    if (data) {
      let off = 0;
      for (let m = 0; m < mips; m++) {
        const mw = Math.max(1, w >> m), mh = Math.max(1, h >> m);
        this.device.queue.writeTexture({ texture: this.base, mipLevel: m }, data,
          { offset: off, bytesPerRow: mw * bpp, rowsPerImage: mh }, [mw, mh]);
        off += mw * mh * bpp;
      }
    }
    this.rebind();
  }

  /** S2: stroke, chunk and point arrays (grown as needed). */
  writeStrokes(strokes: Uint8Array, chunks: Uint8Array, points: Float32Array): void {
    const ensure = (buf: GPUBuffer | undefined, bytes: number): [GPUBuffer, boolean] => {
      if (buf && buf.size >= bytes) return [buf, false];
      buf?.destroy();
      const size = Math.max(256, 2 ** Math.ceil(Math.log2(bytes)));
      return [this.device.createBuffer({ size, usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST }), true];
    };
    const [s, a] = ensure(this.strokeBuf, Math.max(64, strokes.byteLength));
    const [c, b] = ensure(this.chunkBuf, Math.max(32, chunks.byteLength));
    const [p, d] = ensure(this.pointBuf, Math.max(32, points.byteLength));
    this.strokeBuf = s;
    this.chunkBuf = c;
    this.pointBuf = p;
    if (strokes.byteLength) this.device.queue.writeBuffer(s, 0, strokes);
    if (chunks.byteLength) this.device.queue.writeBuffer(c, 0, chunks);
    if (points.byteLength) this.device.queue.writeBuffer(p, 0, points);
    if (a || b || d) this.rebind();
  }

  writeSlotVersions(v: Uint32Array): void {
    this.device.queue.writeBuffer(this.slotVersion!, 0, v);
  }

  private rebind(): void {
    if (!this.atlas || !this.base || !this.pages || !this.slotTime || !this.strokeBuf || !this.slotVersion) return;
    this.bindGroup = this.device.createBindGroup({
      layout: this.pipeline.getBindGroupLayout(0),
      entries: [
        { binding: 0, resource: { buffer: this.uniform } },
        { binding: 1, resource: { buffer: this.pages } },
        { binding: 2, resource: { buffer: this.slotTime } },
        { binding: 3, resource: this.atlas.createView({ dimension: "2d-array" }) },
        { binding: 4, resource: this.base.createView() },
        { binding: 5, resource: this.samplerClamp },
        { binding: 6, resource: this.samplerRepeat },
        { binding: 7, resource: { buffer: this.strokeBuf } },
        { binding: 8, resource: { buffer: this.chunkBuf! } },
        { binding: 9, resource: { buffer: this.pointBuf! } },
        { binding: 10, resource: { buffer: this.slotVersion } },
      ],
    });
  }

  writeTile(slot: number, data: Uint8Array): void {
    const bpp = this.wire === "u8" ? 4 : 8;
    this.device.queue.writeTexture({ texture: this.atlas!, origin: [0, 0, slot] }, data,
      { bytesPerRow: this.slot * bpp, rowsPerImage: this.slot }, [this.slot, this.slot, 1]);
  }

  writePages(pages: Uint32Array): void {
    this.device.queue.writeBuffer(this.pages!, 0, pages);
  }

  writeSlotTimes(t: Float32Array): void {
    this.device.queue.writeBuffer(this.slotTime!, 0, t);
  }

  /** Overrides devicePixelRatio for benchmarks (0 = use the display's ratio). */
  scale = 0;

  resize(): void {
    const dpr = this.scale || window.devicePixelRatio || 1;
    const w = Math.max(1, Math.round(this.canvas.clientWidth * dpr));
    const h = Math.max(1, Math.round(this.canvas.clientHeight * dpr));
    if (this.canvas.width !== w || this.canvas.height !== h) {
      this.canvas.width = w;
      this.canvas.height = h;
    }
  }

  render(p: RenderParams, brush?: BrushParams): void {
    if (!this.bindGroup) return;
    const w = this.canvas.width, h = this.canvas.height;
    const b = basis(p.view);
    const tan = Math.tan(p.view.fov / 2);
    this.uf.set([...b.f, 0, ...b.r, 0, ...b.u, 0, (w / h) * tan, tan, w, h], 0);
    this.uf.set([p.time, p.fade, p.bias, p.cube ? 1 : 0, p.exposure, p.linear ? 1 : 0, p.aniso ? 1 : 0,
      p.debug ? 1 : 0], 16);
    const bc = brush?.cursor;
    this.uf.set([bc?.[0] ?? 0, bc?.[1] ?? 0, bc?.[2] ?? 0, bc ? brush!.radius : 0], 32);
    const bs = brush?.source;
    this.uf.set([bs?.[0] ?? 0, bs?.[1] ?? 0, bs?.[2] ?? 0, bs ? 1 : 0], 36);
    this.uu.set([brush?.strokeCount ?? 0, brush?.baseVersion ?? 0, 0, 0], 40);
    this.device.queue.writeBuffer(this.uniform, 0, this.u);
    const enc = this.device.createCommandEncoder();
    const measure = this.querySet && !this.queryBusy;
    const pass = enc.beginRenderPass({
      colorAttachments: [{ view: this.ctx.getCurrentTexture().createView(), loadOp: "clear", storeOp: "store",
        clearValue: [0, 0, 0, 1] }],
      timestampWrites: measure ? { querySet: this.querySet!, beginningOfPassWriteIndex: 0, endOfPassWriteIndex: 1 } : undefined,
    });
    pass.setPipeline(this.pipeline);
    pass.setBindGroup(0, this.bindGroup);
    pass.draw(3);
    pass.end();
    if (measure) {
      enc.resolveQuerySet(this.querySet!, 0, 2, this.queryResolve!, 0);
      enc.copyBufferToBuffer(this.queryResolve!, 0, this.queryRead!, 0, 16);
    }
    this.device.queue.submit([enc.finish()]);
    if (measure) {
      this.queryBusy = true;
      this.queryRead!.mapAsync(GPUMapMode.READ).then(() => {
        const t = new BigInt64Array(this.queryRead!.getMappedRange());
        this.gpuMs = Number(t[1] - t[0]) / 1e6;
        this.queryRead!.unmap();
        this.queryBusy = false;
      });
    }
  }
}
