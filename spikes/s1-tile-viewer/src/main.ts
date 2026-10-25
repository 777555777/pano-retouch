// Spike S1 – UI wiring, render loop, input, status events and benchmarks.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Renderer, Wire } from "./renderer";
import { Layout, OpenInfo, pct, TileManager, TileOptions } from "./tiles";
import { available, fetchBatch, fetchBytes, initTransports, Transport } from "./transport";
import { View } from "./sphere";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const canvas = $<HTMLCanvasElement>("view");
const logEl = $("log");
const hud = $("hud");

const DEG = Math.PI / 180;
const renderer = new Renderer(canvas);
let info: OpenInfo | null = null;
let tm: TileManager | null = null;
let view: View = { yaw: 0, pitch: 0, fov: 90 * DEG };
let velocity: View = { yaw: 0, pitch: 0, fov: 0 };
let lastView: View = { ...view };
const t0Page = performance.now();

// open timing (per session)
let openT0 = 0;
let baseUploadedAt = 0;
let firstViewMs = NaN;
let tilesMs = NaN;
let sharpMs = NaN;
let rustBaseMs = NaN;
const waiters: Array<() => boolean> = [];

function log(msg: string): void {
  const line = `${((performance.now() - t0Page) / 1000).toFixed(2).padStart(7)}s  ${msg}`;
  console.log(line);
  void invoke("js_log", { msg }).catch(() => {});
  logEl.textContent = (line + "\n" + logEl.textContent).slice(0, 8000);
}

function opts(): TileOptions {
  const wireSel = $<HTMLSelectElement>("wire").value;
  const wire: Wire = wireSel === "auto" ? (info?.storage === "f16" ? "f16" : "u8") : (wireSel as Wire);
  return {
    layout: $<HTMLSelectElement>("layout").value as Layout,
    wire,
    transport: $<HTMLSelectElement>("transport").value as Transport,
    slots: parseInt($<HTMLInputElement>("slots").value),
    maxInflight: parseInt($<HTMLInputElement>("inflight").value),
    uploadsPerFrame: parseInt($<HTMLInputElement>("uploads").value),
    prefetch: $<HTMLInputElement>("prefetch").checked,
    aniso: $<HTMLInputElement>("aniso").checked,
    bias: parseFloat($<HTMLInputElement>("bias").value),
  };
}

async function loadBase(): Promise<void> {
  if (!info) return;
  const o = opts();
  const session = info.session;
  const t = performance.now();
  const { buf } = await fetchBytes(`${session}/base/${o.wire}`, o.transport);
  if (!info || info.session !== session) return;
  let mips = 0;
  for (let w = info.base_w, h = info.base_h; ; w = Math.max(1, w >> 1), h = Math.max(1, h >> 1)) {
    mips++;
    if (w === 1 && h === 1) break;
  }
  renderer.setBase(buf, info.base_w, info.base_h, mips, o.wire);
  baseUploadedAt = performance.now();
  log(`base texture ${(buf.byteLength / 1e6).toFixed(1)} MB fetched+uploaded in ${(baseUploadedAt - t).toFixed(0)} ms`);
}

function rebuildTiles(): void {
  if (!info) return;
  tm?.dispose();
  const ready = tm?.tilesReady ?? false;
  tm = new TileManager(renderer, info, opts());
  tm.onError = log;
  if (ready) tm.onTilesReady();
  if (baseUploadedAt) void loadBase(); // wire format may have changed
}

async function openImage(path: string, useCache: boolean): Promise<OpenInfo> {
  tm?.dispose();
  tm = null;
  info = null;
  baseUploadedAt = 0;
  firstViewMs = tilesMs = sharpMs = rustBaseMs = NaN;
  renderer.setBase(null, 1, 1, 1, "u8");
  openT0 = performance.now();
  const i = await invoke<OpenInfo>("open_image", {
    path,
    useCache,
    writeCache: $<HTMLInputElement>("writecache").checked,
    jpeg: $<HTMLSelectElement>("jpeg").value,
  });
  info = i;
  log(`open #${i.session}: ${i.width}×${i.height} ${i.kind} ${i.storage}, ${i.levels.length} levels, ` +
    `${i.from_cache ? "from cache" : "decoding"} (invoke ${(performance.now() - openT0).toFixed(0)} ms)`);
  tm = new TileManager(renderer, i, opts());
  tm.onError = log;
  view = { yaw: 0, pitch: 0, fov: 90 * DEG };
  drainStatus();
  return i;
}

interface Status { session: number; stage: string; ms: number; detail: string }

// Events can arrive before `open_image` has returned (cache hits are instant), so all events are
// kept and the ones for a session are replayed once its OpenInfo is known.
const statusLog: Status[] = [];
let handledUpTo = 0;

async function handleStatus(s: Status): Promise<void> {
  log(`rust ${s.stage} @ ${s.ms.toFixed(0)} ms – ${s.detail}`);
  if (s.stage === "base") {
    rustBaseMs = s.ms;
    await loadBase();
  } else if (s.stage === "tiles") {
    tilesMs = performance.now() - openT0;
    tm?.onTilesReady();
  } else if (s.stage === "error") {
    log("ERROR " + s.detail);
  }
}

function drainStatus(): void {
  for (; handledUpTo < statusLog.length; handledUpTo++) {
    const s = statusLog[handledUpTo];
    if (!info) return; // wait until the session is known
    if (s.session === info.session) void handleStatus(s);
  }
}

listen<Status>("pano-status", (e) => {
  statusLog.push(e.payload);
  drainStatus();
});

const cacheWritten = (session: number) => statusLog.some((s) => s.session === session && (s.stage === "cache-written" || s.stage === "error"));

// ---------------------------------------------------------------- render loop & metrics

let frameTimes: number[] = [];
let lastFrame = performance.now();
let recording: { frames: number[]; cpu: number[]; gpu: number[]; missingMs: number; maxMissing: number } | null = null;
let script: ((now: number) => boolean) | null = null;
let snapRequest: { name: string; done: () => void } | null = null;
let autoMode = false;

/** Captures the canvas right after the next render (WebGPU canvases are cleared after presentation). */
function snap(name: string): Promise<void> {
  if (!autoMode) return Promise.resolve();
  return new Promise((done) => (snapRequest = { name, done }));
}

function frame(now: number): void {
  const cpu0 = performance.now();
  const dt = now - lastFrame;
  lastFrame = now;
  frameTimes.push(dt);
  if (frameTimes.length > 240) frameTimes.shift();

  if (script && !script(now)) script = null;
  const dts = Math.max(dt, 1) / 1000;
  velocity = {
    yaw: (view.yaw - lastView.yaw) / dts,
    pitch: (view.pitch - lastView.pitch) / dts,
    fov: (view.fov - lastView.fov) / dts,
  };
  lastView = { ...view };

  renderer.resize();
  const time = (now - t0Page) / 1000;
  if (tm) tm.pxScale = renderer.scale || window.devicePixelRatio || 1;
  if (tm) tm.update(view, velocity, canvas.width, canvas.height, now, time);
  renderer.render({
    view,
    time,
    fade: parseFloat($<HTMLInputElement>("fade").value) / 1000,
    bias: tm?.o.bias ?? 0,
    cube: tm?.o.layout === "cube",
    exposure: parseFloat($<HTMLInputElement>("exposure").value),
    linear: info?.storage === "f16",
    aniso: tm?.o.aniso ?? true,
    debug: $<HTMLInputElement>("debug").checked,
  });

  if (snapRequest) {
    const { name, done } = snapRequest;
    snapRequest = null;
    canvas.toBlob(async (b) => {
      if (b) {
        const p = await invoke<string>("save_snapshot", new Uint8Array(await b.arrayBuffer()), { headers: { "x-name": name } });
        log(`snapshot ${p}`);
      }
      done();
    }, "image/png");
  }
  if (baseUploadedAt && isNaN(firstViewMs)) {
    firstViewMs = now - openT0;
    log(`FIRST VIEW ${firstViewMs.toFixed(0)} ms after open`);
  }
  if (tm && tm.tilesReady && isNaN(sharpMs) && tm.stats.missing === 0 && tm.stats.inflight === 0 &&
      tm.stats.queued === 0 && baseUploadedAt) {
    sharpMs = now - openT0;
    log(`SHARP VIEW ${sharpMs.toFixed(0)} ms after open (${tm.stats.needed} tiles)`);
  }
  if (recording && tm) {
    recording.frames.push(dt);
    recording.cpu.push(performance.now() - cpu0);
    if (!isNaN(renderer.gpuMs)) recording.gpu.push(renderer.gpuMs);
    if (tm.stats.missing > 0) recording.missingMs += dt;
    recording.maxMissing = Math.max(recording.maxMissing, tm.stats.missing);
  }
  for (let i = waiters.length - 1; i >= 0; i--) if (waiters[i]()) waiters.splice(i, 1);
  requestAnimationFrame(frame);
}

let peakRss = 0;
let lastBytes = 0;
let lastHud = performance.now();
setInterval(async () => {
  peakRss = (await invoke<{ peak_rss_mb: number }>("stats")).peak_rss_mb;
}, 1000);
setInterval(() => {
  const now = performance.now();
  const ft = frameTimes;
  const avg = ft.reduce((a, b) => a + b, 0) / Math.max(1, ft.length);
  const s = tm?.stats;
  const mbs = tm ? ((tm.bytes - lastBytes) / 1e6) / ((now - lastHud) / 1000) : 0;
  lastBytes = tm?.bytes ?? 0;
  lastHud = now;
  hud.textContent = [
    `${renderer.adapterInfo}`,
    `fps ${(1000 / avg).toFixed(0)}  frame p95 ${pct(ft, 95)?.toFixed(1)} ms  max ${Math.max(...ft).toFixed(1)}  gpu ${renderer.gpuMs.toFixed(2)} ms`,
    `yaw ${(view.yaw / DEG).toFixed(1)}°  pitch ${(view.pitch / DEG).toFixed(1)}°  fov ${(view.fov / DEG).toFixed(2)}°`,
    s ? `centre level ${s.centreLevel}  needed ${s.needed}  missing ${s.missing}  prefetch ${s.prefetch}` : "",
    s ? `resident ${s.resident}/${renderer.slots}  inflight ${s.inflight}  queued ${s.queued}` : "",
    tm ? `fetched ${tm.fetched}  ${mbs.toFixed(0)} MB/s  fetch p50 ${pct(tm.fetchMs, 50)?.toFixed(1)} p95 ${pct(tm.fetchMs, 95)?.toFixed(1)} ms` : "",
    tm ? `tile wait p50 ${pct(tm.waits, 50)?.toFixed(0)} p95 ${pct(tm.waits, 95)?.toFixed(0)} max ${Math.max(0, ...tm.waits).toFixed(0)} ms  dropped ${tm.dropped} evicted ${tm.evicted}` : "",
    `open: first view ${firstViewMs.toFixed(0)} ms (rust base ${rustBaseMs.toFixed(0)})  tiles ${tilesMs.toFixed(0)}  sharp ${sharpMs.toFixed(0)} ms`,
    `rust peak RSS ${peakRss.toFixed(0)} MB`,
  ].join("\n");
}, 250);

// ---------------------------------------------------------------- input

function minFov(): number {
  // 4× beyond 1:1 at level 0
  if (!info) return 1 * DEG;
  const a = Math.PI / info.levels[0].h / 4;
  return 2 * Math.atan((canvas.height / 2) * a);
}
function oneToOneFov(): number {
  if (!info) return 30 * DEG;
  const a = Math.PI / info.levels[0].h;
  return 2 * Math.atan((canvas.height / 2) * a);
}

let drag: { x: number; y: number } | null = null;
canvas.addEventListener("pointerdown", (e) => {
  drag = { x: e.clientX, y: e.clientY };
  canvas.setPointerCapture(e.pointerId);
  script = null;
});
canvas.addEventListener("pointermove", (e) => {
  if (!drag) return;
  const k = (2 * Math.tan(view.fov / 2)) / canvas.clientHeight;
  view.yaw -= (e.clientX - drag.x) * k;
  view.pitch = Math.max(-Math.PI / 2, Math.min(Math.PI / 2, view.pitch + (e.clientY - drag.y) * k));
  drag = { x: e.clientX, y: e.clientY };
});
canvas.addEventListener("pointerup", () => (drag = null));
canvas.addEventListener("wheel", (e) => {
  e.preventDefault();
  view.fov = Math.max(minFov(), Math.min(120 * DEG, view.fov * Math.exp(e.deltaY * 0.0015)));
}, { passive: false });
window.addEventListener("keydown", (e) => {
  if ((e.target as HTMLElement).tagName === "INPUT") return;
  const step = view.fov * (e.shiftKey ? 0.5 : 0.1);
  if (e.key === "ArrowLeft") view.yaw -= step;
  if (e.key === "ArrowRight") view.yaw += step;
  if (e.key === "ArrowUp") view.pitch = Math.min(Math.PI / 2, view.pitch + step);
  if (e.key === "ArrowDown") view.pitch = Math.max(-Math.PI / 2, view.pitch - step);
  if (e.key === "1") view.fov = oneToOneFov();
  if (e.key === "0") view = { yaw: view.yaw, pitch: 0, fov: 90 * DEG };
  if (e.key === "n") view = { yaw: view.yaw, pitch: -Math.PI / 2, fov: 90 * DEG };
  if (e.key === "d") $<HTMLInputElement>("debug").click();
  if (e.key === "h") document.body.classList.toggle("nohud");
});

// ---------------------------------------------------------------- benchmarks

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const until = (cond: () => boolean, timeout = 20000) =>
  new Promise<boolean>((res) => {
    const t = performance.now();
    waiters.push(() => {
      if (cond()) return res(true), true;
      if (performance.now() - t > timeout) {
        log(`wait timed out: ${JSON.stringify(tm?.stats)} missing: ${tm?.missingPaths().slice(0, 12).join(", ")}`);
        return res(false), true;
      }
      return false;
    });
  });

function animate(to: Partial<View>, ms: number): Promise<void> {
  const from = { ...view };
  const target = { ...view, ...to };
  const t = performance.now();
  return new Promise((res) => {
    script = (now) => {
      const k = Math.min(1, (now - t) / ms);
      const e = k < 0.5 ? 2 * k * k : 1 - Math.pow(-2 * k + 2, 2) / 2;
      view = {
        yaw: from.yaw + (target.yaw - from.yaw) * e,
        pitch: from.pitch + (target.pitch - from.pitch) * e,
        // zoom geometrically
        fov: from.fov * Math.pow(target.fov / from.fov, e),
      };
      if (k >= 1) res();
      return k < 1;
    };
  });
}

interface PhaseResult { phase: string; fpsAvg: number; frameP50: number; frameP95: number; frameMax: number;
  longFrames: number; cpuP50: number; cpuP95: number; cpuMax: number; gpuP50: number; gpuP95: number; blurryMs: number; maxMissing: number; tileWaitP95: number; tileWaitMax: number }

async function phase(name: string, run: () => Promise<void>): Promise<PhaseResult> {
  tm?.resetMetrics();
  recording = { frames: [], cpu: [], gpu: [], missingMs: 0, maxMissing: 0 };
  await run();
  const r = recording;
  recording = null;
  const f = r.frames;
  const res: PhaseResult = {
    phase: name,
    fpsAvg: Math.round(1000 / (f.reduce((a, b) => a + b, 0) / f.length)),
    frameP50: +pct(f, 50).toFixed(1),
    frameP95: +pct(f, 95).toFixed(1),
    frameMax: +Math.max(...f).toFixed(1),
    longFrames: f.filter((x) => x > 25).length,
    cpuP50: +pct(r.cpu, 50).toFixed(2),
    cpuP95: +pct(r.cpu, 95).toFixed(2),
    cpuMax: +Math.max(...r.cpu).toFixed(2),
    gpuP50: +pct(r.gpu, 50)?.toFixed(2),
    gpuP95: +pct(r.gpu, 95)?.toFixed(2),
    blurryMs: Math.round(r.missingMs),
    maxMissing: r.maxMissing,
    tileWaitP95: Math.round(pct(tm?.waits ?? [], 95) || 0),
    tileWaitMax: Math.round(Math.max(0, ...(tm?.waits ?? []))),
  };
  log(`  ${name}: ${JSON.stringify(res)}`);
  return res;
}

const tag = () => `${info?.kind}-${info?.storage}-${tm?.o.layout}`;

async function benchPath(): Promise<PhaseResult[]> {
  if (!tm) throw new Error("open an image first");
  log(`bench path – ${tm.o.layout}, ${tm.o.wire}, ${tm.o.transport}, ${canvas.width}×${canvas.height} px`);
  view = { yaw: 0, pitch: 0, fov: 90 * DEG };
  await until(() => !!tm && tm.stats.missing === 0 && tm.stats.inflight === 0);
  const out: PhaseResult[] = [];
  await snap(`${tag()}-horizon`);
  out.push(await phase("pan 360° @ fov 90 (6 s)", () => animate({ yaw: view.yaw + 2 * Math.PI }, 6000)));
  out.push(await phase("tilt to nadir (1.5 s) + settle", async () => {
    await animate({ pitch: -Math.PI / 2 }, 1500);
    await sleep(1000);
  }));
  await snap(`${tag()}-nadir`);
  out.push(await phase("zoom to 1:1 at nadir (2 s) + settle", async () => {
    await animate({ fov: oneToOneFov() }, 2000);
    await sleep(1500);
  }));
  await snap(`${tag()}-nadir-1to1`);
  out.push(await phase("tilt to horizon at 1:1 (2 s)", () => animate({ pitch: 0 }, 2000)));
  out.push(await phase("pan 60° at 1:1 (3 s) + settle", async () => {
    await animate({ yaw: view.yaw + 60 * DEG }, 3000);
    await sleep(1000);
  }));
  out.push(await phase("zoom out to fov 100 (1.5 s)", () => animate({ fov: 100 * DEG }, 1500)));
  console.table(out);
  return out;
}

async function benchTransport(): Promise<unknown[]> {
  if (!info || !tm?.tilesReady) throw new Error("open an image and wait for tiles");
  const l0 = info.levels[0];
  const session = info.session;
  const N = 64;
  const tiles = (): [number, number, number][] =>
    Array.from({ length: N }, (_, i) => [0, (i * 7) % l0.tiles_x, Math.floor(l0.tiles_y / 2) - 2 + (i % 4)]);
  // each case: requests to make, how to make one, tiles per request
  const cases: { name: string; conc: number; n: number; per: number; run: (i: number, t: Transport) => Promise<number> }[] = [
    { name: "null 1 MB", conc: 1, n: N, per: 1, run: async (_i, t) => (await fetchBytes("null/1056784", t)).buf.byteLength },
    { name: "null 1 MB", conc: 8, n: N, per: 1, run: async (_i, t) => (await fetchBytes("null/1056784", t)).buf.byteLength },
    ...(["u8", "f16"] as const).flatMap((wire) => [1, 8].map((conc) => ({
      name: `tile ${wire}`, conc, n: N, per: 1,
      run: async (i: number, t: Transport) => {
        const [l, x, y] = tiles()[i];
        return (await fetchBytes(`${session}/e/${l}/${x}/${y}/${wire}`, t)).buf.byteLength;
      },
    }))),
    { name: "batch 8 × tile u8", conc: 2, n: N / 8, per: 8, run: async (i, t) => {
      const parts = await fetchBatch(session, "u8", tiles().slice(i * 8, i * 8 + 8), t);
      return parts.reduce((a, p) => a + p.byteLength, 0);
    } },
  ];
  const rows = [];
  for (const transport of available) {
    for (const c of cases) {
      let bytes = 0, next = 0;
      const lat: number[] = [];
      const t = performance.now();
      await Promise.all(Array.from({ length: c.conc }, async () => {
        while (next < c.n) {
          const i = next++;
          const s = performance.now();
          const n = await c.run(i, transport); // not `bytes += await …` – that loses updates when parallel
          bytes += n;
          lat.push(performance.now() - s);
        }
      }));
      const el = (performance.now() - t) / 1000;
      const row = { transport, payload: c.name, concurrency: c.conc, MBps: Math.round(bytes / 1e6 / el),
        tilesPerSec: Math.round((c.n * c.per) / el), latP50: +pct(lat, 50).toFixed(1), latP95: +pct(lat, 95).toFixed(1) };
      rows.push(row);
      log(`  ${JSON.stringify(row)}`);
    }
  }
  console.table(rows);
  return rows;
}

async function benchOpen(path: string): Promise<unknown[]> {
  const rows = [];
  await invoke("clear_cache");
  for (const run of ["decode", "cache"]) {
    await openImage(path, run === "cache");
    const ok = await until(() => !isNaN(sharpMs), 30000);
    const row = { run, firstView: Math.round(firstViewMs), rustBase: Math.round(rustBaseMs),
      tiles: Math.round(tilesMs), sharp: ok ? Math.round(sharpMs) : "timeout", peakRssMb: Math.round(peakRss) };
    rows.push(row);
    log(`  open ${JSON.stringify(row)}`);
    if (run === "decode") {
      log("  waiting for cache write…");
      const session = info!.session;
      await until(() => cacheWritten(session), 60000);
    }
  }
  console.table(rows);
  return rows;
}

async function benchOpenCached(path: string): Promise<unknown> {
  await openImage(path, true);
  const ok = await until(() => !isNaN(sharpMs), 30000);
  const row = { run: "cache", transport: tm?.o.transport, firstView: Math.round(firstViewMs), sharp: ok ? Math.round(sharpMs) : "timeout" };
  log(`  open ${JSON.stringify(row)}`);
  return row;
}

// ---------------------------------------------------------------- UI

async function main(): Promise<void> {
  try {
    await renderer.init();
    log(`WebGPU OK: ${renderer.adapterInfo}, maxTextureArrayLayers ${renderer.maxLayers}, features: ${renderer.features}`);
  } catch (e) {
    log(`WebGPU init failed: ${e}`);
    hud.textContent = String(e);
    return;
  }
  const { available: tr, notes } = await initTransports();
  log(`transports: ${tr.join(", ")}${notes.length ? ` (${notes.join("; ")})` : ""}`);
  const tsel = $<HTMLSelectElement>("transport");
  const labels: Record<Transport, string> = { protocol: "custom URI scheme", ipc: "ipc::Response", ws: "loopback WebSocket", shared: "WebView2 shared buffers" };
  tsel.innerHTML = tr.map((t) => `<option value="${t}">${labels[t]}</option>`).join("");
  const files = await invoke<{ path: string; name: string; size_mb: number }[]>("list_testdata");
  const sel = $<HTMLSelectElement>("file");
  for (const f of files) {
    const o = document.createElement("option");
    o.value = f.path;
    // Show only the file name in the UI (paths may reveal private folder names in screenshots).
    o.textContent = `${f.name.split("/").pop()} (${f.size_mb.toFixed(0)} MB)`;
    sel.append(o);
  }
  log(`${files.length} test files found`);
  const guard = (fn: () => Promise<unknown>) => async () => {
    try {
      const r = await fn();
      if (r) (window as unknown as { lastBench: unknown }).lastBench = r;
    } catch (e) {
      log(`ERROR ${e}`);
    }
  };
  $("open").onclick = guard(() => openImage(sel.value, $<HTMLInputElement>("usecache").checked));
  $("clear").onclick = guard(async () => {
    await invoke("clear_cache");
    log("cache cleared");
  });
  $("bpath").onclick = guard(benchPath);
  $("btransport").onclick = guard(benchTransport);
  $("bopen").onclick = guard(() => benchOpen(sel.value));
  $("copy").onclick = () => navigator.clipboard.writeText(logEl.textContent ?? "");
  for (const id of ["layout", "wire", "transport", "slots", "inflight", "uploads", "prefetch", "aniso", "bias"]) {
    $(id).addEventListener("change", () => {
      rebuildTiles();
      log(`tiles reset: ${JSON.stringify(tm?.o)}`);
    });
  }
  requestAnimationFrame(frame);

  const auto = await invoke<{ files: string[]; layouts: Layout[]; scale: number; transports: Transport[]; snaps: boolean; tune: [string, string][] } | null>("auto_config");
  if (auto) {
    autoMode = auto.snaps;
    renderer.scale = auto.scale;
    for (const [k, v] of auto.tune) {
      const el = $<HTMLInputElement>(k);
      if (el.type === "checkbox") el.checked = v === "1" || v === "true";
      else el.value = v;
    }
    if (auto.tune.length) log(`tuning: ${JSON.stringify(auto.tune)}`);
    await sleep(500);
    const results: Record<string, unknown> = {
      adapter: renderer.adapterInfo,
      canvas: `${canvas.width}×${canvas.height}`,
      dpr: window.devicePixelRatio,
      userAgent: navigator.userAgent,
      transports: available,
    };
    const transports = auto.transports.filter((t) => available.includes(t));
    for (const [i, f] of auto.files.entries()) {
      const name = f.split(/[\\/]/).pop()!;
      sel.value = f;
      log(`AUTO ${name}`);
      const r: Record<string, unknown> = {};
      try {
        $<HTMLSelectElement>("layout").value = "equirect";
        $<HTMLSelectElement>("transport").value = transports[0];
        r.open = await benchOpen(f);
        for (const transport of transports) {
          $<HTMLSelectElement>("transport").value = transport;
          r[`open-${transport}`] = await benchOpenCached(f);
          for (const layout of auto.layouts) {
            $<HTMLSelectElement>("layout").value = layout;
            rebuildTiles();
            r[`path-${layout}-${transport}`] = await benchPath();
            $<HTMLInputElement>("debug").checked = true;
            view = { yaw: 0, pitch: -Math.PI / 2, fov: 90 * DEG };
            await until(() => !!tm && tm.stats.missing === 0 && tm.stats.inflight === 0, 5000);
            await snap(`${tag()}-${transport}-nadir-levels`);
            $<HTMLInputElement>("debug").checked = false;
          }
        }
        if (i === 0) {
          $<HTMLSelectElement>("layout").value = "equirect";
          $<HTMLSelectElement>("transport").value = transports[0];
          rebuildTiles();
          r.transport = await benchTransport();
        }
      } catch (e) {
        r.error = String(e);
        log(`ERROR ${e}`);
      }
      r.peakRssMb = Math.round(peakRss);
      results[name] = r;
    }
    log("AUTO RESULT " + JSON.stringify(results));
    await sleep(300);
    await invoke("quit");
  }
}

void main();
