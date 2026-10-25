//! Spike S1 – Tauri glue: commands, status events and the tile transports
//! (custom URI scheme `tile://`, `ipc::Response`, and in S1b: batching, loopback WebSocket and
//! WebView2 shared buffers).

pub mod cache;
pub mod decode;
pub mod edit;
pub mod pyramid;
pub mod session;
pub mod synthetic;
pub mod transport;

// S1b: fresh large allocations are expensive on Windows (page faults, zeroing); mimalloc reuses memory.
#[cfg(feature = "native")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use parking_lot::Mutex;
use pyramid::Wire;
use serde::Serialize;
use session::{Session, TileError};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;
use tauri::{Emitter, Manager};

#[derive(Default)]
struct AppState {
    current: Mutex<Option<Arc<Session>>>,
    next_id: AtomicU32,
    ws: Mutex<Option<(u16, String)>>,
    shared: transport::SharedSlots,
}

#[derive(Serialize)]
struct TestFile {
    path: String,
    name: String,
    size_mb: f64,
}

pub fn testdata_root() -> PathBuf {
    std::env::var_os("PANO_TESTDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../testdata-local"))
}

/// Panoramas under `root`, skipping hidden/underscore folders and source frames (RAW, Stitching).
pub fn list_images(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            let name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if name.starts_with('.') {
                continue;
            }
            if p.is_dir() {
                if depth < 4 && !name.starts_with('_') && name != "RAW" && name != "Stitching" {
                    walk(&p, depth + 1, out);
                }
            } else if decode::kind_of(&p).is_some() {
                out.push(p);
            }
        }
    }
    let mut out = vec![];
    walk(root, 0, &mut out);
    out
}

#[tauri::command]
fn list_testdata() -> Vec<TestFile> {
    let root = testdata_root();
    let synthetic = [24000u32, 8192].map(|w| TestFile {
        path: format!("{}{w}", synthetic::PREFIX),
        name: format!(
            "synthetic {w}×{} ({:.0} MP, public)",
            w / 2,
            w as f64 * (w / 2) as f64 / 1e6
        ),
        size_mb: 0.0,
    });
    synthetic
        .into_iter()
        .chain(list_images(&root).into_iter().map(|p| TestFile {
            size_mb: std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0) as f64 / 1e6,
            name: p.strip_prefix(&root).unwrap_or(&p).to_string_lossy().into(),
            path: p.to_string_lossy().into(),
        }))
        .collect()
}

#[tauri::command]
fn open_image(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    path: String,
    use_cache: bool,
    write_cache: bool,
    jpeg: String,
) -> Result<session::OpenInfo, String> {
    let id = state.next_id.fetch_add(1, Ordering::SeqCst) + 1;
    // Drop the previous image first so its memory can be released.
    *state.current.lock() = None;
    let opts = session::Options {
        use_cache,
        write_cache,
        jpeg: if jpeg == "zune" {
            decode::JpegDecoder::Zune
        } else {
            decode::JpegDecoder::Turbo
        },
    };
    let app2 = app.clone();
    let s = Session::open(id, PathBuf::from(path), opts, move |st| {
        eprintln!(
            "[session {}] {:>8.1} ms  {:<14} {}",
            st.session, st.ms, st.stage, st.detail
        );
        let _ = app2.emit("pano-status", st);
    })?;
    let info = s.info.clone();
    *state.current.lock() = Some(s);
    Ok(info)
}

#[derive(Serialize)]
struct Stats {
    peak_rss_mb: f64,
}

#[tauri::command]
fn stats() -> Stats {
    Stats {
        peak_rss_mb: session::peak_rss_mb(),
    }
}

/// Mirrors the frontend log to stdout so benchmark runs can be read from the terminal.
#[tauri::command]
fn js_log(msg: String) {
    println!("[ui] {msg}");
}

#[derive(Serialize)]
struct AutoConfig {
    files: Vec<String>,
    layouts: Vec<String>,
    scale: f64,
    transports: Vec<String>,
    /// canvas snapshots only when `S1_SNAP_DIR` is set (they show the private test images)
    snaps: bool,
    /// `S1_AUTO_MODE=brush` runs the S2 brush benchmark instead of the S1 viewer benchmarks
    mode: String,
    /// `S2_ONLY=<part of a scenario name>`: run only matching brush scenarios
    only: String,
    /// UI control overrides, e.g. `S1_TUNE=inflight=24,uploads=12`.
    tune: Vec<(String, String)>,
}

/// Automatic benchmark mode: `S1_AUTO=<comma-separated file-name substrings>` (optionally
/// `S1_AUTO_LAYOUTS=equirect,cube`). The UI runs all benchmarks, logs the results and quits.
#[tauri::command]
fn auto_config() -> Option<AutoConfig> {
    let files = std::env::var("S1_AUTO").ok()?;
    let all = list_images(&testdata_root());
    let files = files
        .split(',')
        .filter_map(|pat| {
            let pat = pat.trim();
            if pat.starts_with(synthetic::PREFIX) {
                return Some(pat.to_string());
            }
            all.iter()
                .find(|p| p.to_string_lossy().contains(pat))
                .map(|p| p.to_string_lossy().to_string())
        })
        .collect();
    let layouts = std::env::var("S1_AUTO_LAYOUTS").unwrap_or_else(|_| "equirect".into());
    let transports =
        std::env::var("S1_AUTO_TRANSPORTS").unwrap_or_else(|_| "protocol,ws,shared".into());
    let scale = std::env::var("S1_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let tune = std::env::var("S1_TUNE")
        .unwrap_or_default()
        .split(',')
        .filter_map(|kv| {
            kv.split_once('=')
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        })
        .collect();
    Some(AutoConfig {
        files,
        layouts: layouts.split(',').map(|s| s.trim().to_string()).collect(),
        scale,
        transports: transports
            .split(',')
            .map(|s| s.trim().to_string())
            .collect(),
        snaps: std::env::var_os("S1_SNAP_DIR").is_some(),
        mode: std::env::var("S1_AUTO_MODE").unwrap_or_default(),
        only: std::env::var("S2_ONLY").unwrap_or_default(),
        tune,
    })
}

/// Saves a PNG of the canvas (raw request body) to `S1_SNAP_DIR` – local debugging only, never commit.
#[tauri::command]
fn save_snapshot(request: tauri::ipc::Request<'_>) -> Result<String, String> {
    let tauri::ipc::InvokeBody::Raw(data) = request.body() else {
        return Err("expected raw body".into());
    };
    let name = request
        .headers()
        .get("x-name")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("snap");
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let dir = std::env::var_os("S1_SNAP_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| cache::root().join("snapshots"));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let p = dir.join(format!("{name}.png"));
    std::fs::write(&p, data).map_err(|e| e.to_string())?;
    Ok(p.to_string_lossy().into())
}

#[tauri::command]
fn quit(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn clear_cache(state: tauri::State<'_, AppState>) -> Result<(), String> {
    *state.current.lock() = None;
    cache::clear_all()
}

/// Request: `<session>/base/<fmt>` | `<session>/e/<level>/<x>/<y>/<fmt>` | `<session>/c/<face>/<level>/<x>/<y>/<fmt>`
/// | `<session>/batch/<fmt>/<level>.<x>.<y>_<level>.<x>.<y>…` | `null/<bytes>` (transport only, no tile work)
fn serve(state: &AppState, path: &str) -> Result<Vec<u8>, (u16, String)> {
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let bad = |m: &str| (400u16, m.to_string());
    if parts.first() == Some(&"null") {
        let n = parts
            .get(1)
            .and_then(|p| p.parse().ok())
            .ok_or_else(|| bad("bad size"))?;
        return Ok(transport::null_payload(n));
    }
    if parts.get(1) == Some(&"batch") {
        let session = parts[0];
        let fmt = parts.get(2).copied().unwrap_or("");
        let list = parts.get(3).copied().unwrap_or("");
        let items = list
            .split('_')
            .map(|t| {
                let p: Vec<&str> = t.split('.').collect();
                if p.len() != 3 {
                    return Err(bad("bad batch item"));
                }
                serve(
                    state,
                    &format!("{session}/e/{}/{}/{}/{fmt}", p[0], p[1], p[2]),
                )
            })
            .collect();
        return Ok(transport::pack_batch(items));
    }
    let num = |i: usize| -> Result<u32, (u16, String)> {
        parts
            .get(i)
            .and_then(|p| p.parse().ok())
            .ok_or_else(|| bad("bad number"))
    };
    let wire = Wire::parse(parts.last().copied().unwrap_or("")).ok_or_else(|| bad("bad format"))?;
    let session_id = num(0)?;
    let s = state
        .current
        .lock()
        .clone()
        .ok_or((410, "no image".to_string()))?;
    if s.info.session != session_id {
        return Err((410, "stale session".into()));
    }
    let r = match parts.get(1).copied() {
        Some("base") => s.base_wire(wire),
        Some("e") => s.equirect_tile(num(2)? as usize, num(3)?, num(4)?, wire),
        Some("c") => s.cube_tile(num(2)?, num(3)? as usize, num(4)?, num(5)?, wire),
        _ => return Err(bad("bad path")),
    };
    r.map_err(|e| match e {
        TileError::NotReady => (503, "not ready".into()),
        TileError::Bad(m) => (400, m),
    })
}

/// S2: commit a stroke (full-resolution rasterisation in Rust).
#[tauri::command]
async fn commit_stroke(
    state: tauri::State<'_, AppState>,
    session: u32,
    stroke: edit::Stroke,
) -> Result<session::CommitResult, String> {
    let s = state.current.lock().clone().ok_or("no image")?;
    if s.info.session != session {
        return Err("stale session".into());
    }
    tauri::async_runtime::spawn_blocking(move || s.commit(&stroke))
        .await
        .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
struct WsInfo {
    port: u16,
    token: String,
}

#[tauri::command]
fn ws_info(state: tauri::State<'_, AppState>) -> Option<WsInfo> {
    state
        .ws
        .lock()
        .clone()
        .map(|(port, token)| WsInfo { port, token })
}

/// Creates one WebView2 shared buffer per entry of `sizes` and posts them to the page (Windows only).
#[tauri::command]
async fn shared_init(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    sizes: Vec<usize>,
) -> Result<usize, String> {
    let slots =
        tauri::async_runtime::spawn_blocking(move || transport::create_shared(&webview, sizes))
            .await
            .map_err(|e| e.to_string())??;
    let state = app.state::<AppState>();
    let count = slots.len();
    *state.shared.0.lock() = slots;
    Ok(count)
}

/// Serves `path` into shared slot `slot`; returns the payload length.
#[tauri::command]
async fn tile_shared(
    state: tauri::State<'_, AppState>,
    path: String,
    slot: usize,
) -> Result<u32, String> {
    let bytes = serve(&state, &path).map_err(|(code, m)| format!("{code} {m}"))?;
    transport::write_shared(&state.shared, slot, &bytes)
}

#[tauri::command]
async fn tile_ipc(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<tauri::ipc::Response, String> {
    serve(&state, &path)
        .map(tauri::ipc::Response::new)
        .map_err(|(code, m)| format!("{code} {m}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .setup(|app| {
            // S1b: loopback WebSocket transport (127.0.0.1, random port, random token)
            let handle = app.handle().clone();
            let token = transport::ws::token();
            let (tx, rx) = tokio::sync::oneshot::channel();
            let h2 = handle.clone();
            let handler: transport::ws::Handler =
                Arc::new(move |path: &str| serve(&h2.state::<AppState>(), path));
            let t2 = token.clone();
            tauri::async_runtime::spawn(async move { transport::ws::serve(t2, handler, tx).await });
            tauri::async_runtime::spawn(async move {
                if let Ok(port) = rx.await {
                    *handle.state::<AppState>().ws.lock() = Some((port, token));
                }
            });
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("tile", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_string();
            tauri::async_runtime::spawn_blocking(move || {
                let t = Instant::now();
                let state = app.state::<AppState>();
                let res = serve(&state, &path);
                let ms = format!("{:.2}", t.elapsed().as_secs_f64() * 1e3);
                let b = tauri::http::Response::builder()
                    .header("Access-Control-Allow-Origin", "*")
                    .header("Access-Control-Expose-Headers", "x-server-ms")
                    .header("x-server-ms", ms);
                let resp = match res {
                    Ok(bytes) => b
                        .status(200)
                        .header("Content-Type", "application/octet-stream")
                        .body(bytes),
                    Err((code, msg)) => b.status(code).body(msg.into_bytes()),
                };
                responder.respond(resp.unwrap());
            });
        })
        .invoke_handler(tauri::generate_handler![
            list_testdata,
            open_image,
            stats,
            clear_cache,
            tile_ipc,
            js_log,
            auto_config,
            save_snapshot,
            quit,
            ws_info,
            shared_init,
            tile_shared,
            commit_stroke
        ])
        .run(context())
        .expect("error while running tauri application");
}

#[cfg(not(s1_xcheck))]
fn context() -> tauri::Context {
    tauri::generate_context!()
}

// Cross type-check from macOS only (the macro embeds host-specific items).
#[cfg(s1_xcheck)]
fn context() -> tauri::Context {
    unimplemented!()
}
