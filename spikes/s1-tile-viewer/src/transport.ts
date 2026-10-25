// Tile transports from Rust into the page. S1: custom URI scheme and ipc::Response.
// S1b: loopback WebSocket and WebView2 shared buffers (Windows), plus batching for the benchmark.

import { invoke, convertFileSrc } from "@tauri-apps/api/core";

export type Transport = "protocol" | "ipc" | "ws" | "shared";

export class NotReady extends Error {}

const tileBaseUrl = convertFileSrc("", "tile");

export interface Fetched {
  buf: Uint8Array;
  serverMs: number;
}

const failed = (msg: string): Error => (msg.startsWith("503") ? new NotReady() : new Error(msg));

// ---------------------------------------------------------------- WebSocket

let ws: WebSocket | null = null;
let wsNext = 1;
const wsPending = new Map<number, { res: (f: Fetched) => void; rej: (e: Error) => void }>();

async function connectWs(): Promise<boolean> {
  let info: { port: number; token: string } | null = null;
  for (let i = 0; i < 20 && !info; i++) {
    info = await invoke("ws_info");
    if (!info) await new Promise((r) => setTimeout(r, 100));
  }
  if (!info) return false;
  const sock = new WebSocket(`ws://127.0.0.1:${info.port}/?token=${info.token}`);
  sock.binaryType = "arraybuffer";
  sock.onmessage = (e) => {
    const b = e.data as ArrayBuffer;
    const head = new Uint32Array(b, 0, 2);
    const p = wsPending.get(head[0]);
    if (!p) return;
    wsPending.delete(head[0]);
    if (head[1] === 200) p.res({ buf: new Uint8Array(b, 8), serverMs: NaN });
    else p.rej(failed(`${head[1]} ${new TextDecoder().decode(new Uint8Array(b, 8))}`));
  };
  sock.onclose = () => {
    for (const p of wsPending.values()) p.rej(new Error("ws closed"));
    wsPending.clear();
    ws = null;
  };
  const ok = await new Promise<boolean>((res) => {
    sock.onopen = () => res(true);
    sock.onerror = () => res(false);
  });
  if (ok) ws = sock;
  return ok;
}

function fetchWs(path: string): Promise<Fetched> {
  if (!ws) return Promise.reject(new Error("ws not connected"));
  const id = wsNext++;
  return new Promise((res, rej) => {
    wsPending.set(id, { res, rej });
    ws!.send(`${id} ${path}`);
  });
}

// ---------------------------------------------------------------- WebView2 shared buffers

interface WebView2 {
  addEventListener(type: "sharedbufferreceived", cb: (e: { getBuffer(): ArrayBuffer; additionalData: { slot: number } }) => void): void;
}
const webview2 = (window as unknown as { chrome?: { webview?: WebView2 } }).chrome?.webview;

const BIG_SLOT = 0; // for base textures and batches
const BIG_SIZE = 32 << 20;
const TILE_SIZE = 514 * 514 * 8; // f16 tile
const sharedBufs: ArrayBuffer[] = [];
const freeTileSlots: number[] = [];
let bigBusy = false;
const slotWaiters: Array<() => void> = [];
webview2?.addEventListener("sharedbufferreceived", (e) => {
  sharedBufs[e.additionalData.slot] = e.getBuffer();
});

async function initShared(tileSlots: number): Promise<boolean> {
  if (!webview2) return false;
  const sizes = [BIG_SIZE, ...Array.from({ length: tileSlots }, () => TILE_SIZE)];
  const n = await invoke<number>("shared_init", { sizes });
  for (let i = 0; i < 50 && sharedBufs.filter(Boolean).length < n; i++) await new Promise((r) => setTimeout(r, 20));
  if (sharedBufs.filter(Boolean).length < n) return false;
  for (let i = 1; i < n; i++) freeTileSlots.push(i);
  return true;
}

async function acquire(big: boolean): Promise<number> {
  for (;;) {
    if (big && !bigBusy) {
      bigBusy = true;
      return BIG_SLOT;
    }
    if (!big && freeTileSlots.length) return freeTileSlots.pop()!;
    await new Promise<void>((r) => slotWaiters.push(r));
  }
}

function release(slot: number): void {
  if (slot === BIG_SLOT) bigBusy = false;
  else freeTileSlots.push(slot);
  slotWaiters.shift()?.();
}

async function fetchShared(path: string): Promise<Fetched> {
  const big = path.includes("/base/") || path.includes("/batch/") || path.startsWith("null/");
  const slot = await acquire(big);
  try {
    const len = await invoke<number>("tile_shared", { path, slot });
    // copy out so the slot can be reused immediately (memcpy, ~0.1 ms per MB)
    return { buf: new Uint8Array(sharedBufs[slot], 0, len).slice(), serverMs: NaN };
  } catch (e) {
    throw failed(String(e));
  } finally {
    release(slot);
  }
}

// ---------------------------------------------------------------- public API

export const available: Transport[] = ["protocol", "ipc"];

/** Connects the optional transports; returns the ones that work in this webview and why others don't. */
export async function initTransports(): Promise<{ available: Transport[]; notes: string[] }> {
  const notes: string[] = [];
  const ok = await connectWs().catch((e) => (notes.push(`ws: ${e}`), false));
  if (ok) available.push("ws");
  else notes.push("ws: not connected");
  if (!webview2) notes.push("shared: no WebView2 (only on Windows)");
  else if (await initShared(48).catch((e) => (notes.push(`shared: ${e}`), false))) available.push("shared");
  return { available, notes };
}

export async function fetchBytes(path: string, transport: Transport, signal?: AbortSignal): Promise<Fetched> {
  switch (transport) {
    case "ipc":
      try {
        return { buf: new Uint8Array(await invoke<ArrayBuffer>("tile_ipc", { path })), serverMs: NaN };
      } catch (e) {
        throw failed(String(e));
      }
    case "ws":
      return fetchWs(path);
    case "shared":
      return fetchShared(path);
    default: {
      const r = await fetch(tileBaseUrl + path, { signal });
      if (r.status === 503) throw new NotReady();
      if (!r.ok) throw new Error(`${r.status} ${await r.text()}`);
      return { buf: new Uint8Array(await r.arrayBuffer()), serverMs: parseFloat(r.headers.get("x-server-ms") ?? "NaN") };
    }
  }
}

/** Several equirect tiles in one response (`[u32 len][u32 status][bytes padded to 8]` each). */
export async function fetchBatch(session: number, wire: string, tiles: [number, number, number][], transport: Transport): Promise<Uint8Array[]> {
  const list = tiles.map(([l, x, y]) => `${l}.${x}.${y}`).join("_");
  const { buf } = await fetchBytes(`${session}/batch/${wire}/${list}`, transport);
  const out: Uint8Array[] = [];
  const dv = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  for (let o = 0; o < buf.byteLength;) {
    const len = dv.getUint32(o, true);
    o += 8;
    out.push(buf.subarray(o, o + len));
    o += Math.ceil(len / 8) * 8;
  }
  return out;
}
