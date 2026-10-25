//! Spike S1b – alternative tile transports: batching, loopback WebSocket and (Windows only) WebView2
//! shared buffers. All of them carry the same bytes that `serve()` produces.

use std::sync::OnceLock;

/// Pure-transport payload without any tile work: `null/<bytes>`.
pub fn null_payload(n: usize) -> Vec<u8> {
    static ZEROS: OnceLock<Vec<u8>> = OnceLock::new();
    let z = ZEROS.get_or_init(|| vec![7u8; 8 << 20]);
    z[..n.min(z.len())].to_vec()
}

/// Several results in one body: per item `[u32 len][u32 status][bytes, padded to 8]`.
pub fn pack_batch(items: Vec<Result<Vec<u8>, (u16, String)>>) -> Vec<u8> {
    let total: usize = items
        .iter()
        .map(|r| 8 + r.as_ref().map(|b| b.len().next_multiple_of(8)).unwrap_or(0))
        .sum();
    let mut out = Vec::with_capacity(total);
    for r in items {
        match r {
            Ok(b) => {
                out.extend_from_slice(&(b.len() as u32).to_le_bytes());
                out.extend_from_slice(&200u32.to_le_bytes());
                out.extend_from_slice(&b);
                out.resize(out.len().next_multiple_of(8), 0);
            }
            Err((code, _)) => {
                out.extend_from_slice(&0u32.to_le_bytes());
                out.extend_from_slice(&(code as u32).to_le_bytes());
            }
        }
    }
    out
}

pub mod ws {
    //! Loopback WebSocket: client sends text `"<id> <path>"`, server answers with a binary message
    //! `[u32 id][u32 status][payload]`. Bound to 127.0.0.1 on a random port, guarded by a random token.

    use futures_util::{SinkExt, StreamExt};
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};

    pub type Handler = Arc<dyn Fn(&str) -> Result<Vec<u8>, (u16, String)> + Send + Sync>;

    pub fn token() -> String {
        use std::hash::{BuildHasher, Hasher};
        let mut s = String::new();
        for _ in 0..2 {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u128(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
            );
            s.push_str(&format!("{:016x}", h.finish()));
        }
        s
    }

    pub async fn serve(token: String, handler: Handler, ready: tokio::sync::oneshot::Sender<u16>) {
        let listener = match TcpListener::bind("127.0.0.1:0").await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("ws: bind failed: {e}");
                return;
            }
        };
        let _ = ready.send(listener.local_addr().unwrap().port());
        while let Ok((stream, _)) = listener.accept().await {
            let _ = stream.set_nodelay(true);
            let (token, handler) = (token.clone(), handler.clone());
            tokio::spawn(async move {
                let check = |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
                    let ok = req
                        .uri()
                        .query()
                        .is_some_and(|q| q == format!("token={token}"));
                    if ok {
                        Ok(resp)
                    } else {
                        let mut r = ErrorResponse::new(Some("forbidden".into()));
                        *r.status_mut() =
                            tokio_tungstenite::tungstenite::http::StatusCode::FORBIDDEN;
                        Err(r)
                    }
                };
                let Ok(ws) = tokio_tungstenite::accept_hdr_async(stream, check).await else {
                    return;
                };
                let (mut sink, mut source) = ws.split();
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
                let writer = tokio::spawn(async move {
                    while let Some(msg) = rx.recv().await {
                        if sink.send(Message::Binary(msg.into())).await.is_err() {
                            break;
                        }
                    }
                });
                while let Some(Ok(msg)) = source.next().await {
                    let Message::Text(text) = msg else { continue };
                    let Some((id, path)) = text.split_once(' ') else {
                        continue;
                    };
                    let id: u32 = id.parse().unwrap_or(0);
                    let (path, handler, tx) = (path.to_string(), handler.clone(), tx.clone());
                    tokio::task::spawn_blocking(move || {
                        let r = handler(&path);
                        let (status, body) = match r {
                            Ok(b) => (200u32, b),
                            Err((c, m)) => (c as u32, m.into_bytes()),
                        };
                        let mut out = Vec::with_capacity(8 + body.len());
                        out.extend_from_slice(&id.to_le_bytes());
                        out.extend_from_slice(&status.to_le_bytes());
                        out.extend_from_slice(&body);
                        let _ = tx.send(out);
                    });
                }
                writer.abort();
            });
        }
    }
}

/// Raw pointers to the shared buffers that were handed to the page: (address, size).
#[derive(Default)]
pub struct SharedSlots(pub parking_lot::Mutex<Vec<(usize, usize)>>);

/// Writes `bytes` into shared slot `slot`. The page reads it after the command returns.
pub fn write_shared(slots: &SharedSlots, slot: usize, bytes: &[u8]) -> Result<u32, String> {
    let (ptr, size) = *slots.0.lock().get(slot).ok_or("bad slot")?;
    if bytes.len() > size {
        return Err(format!("payload {} > slot size {size}", bytes.len()));
    }
    // SAFETY: the mapping stays alive for the whole process (buffers are leaked on purpose) and the
    // page only reads a slot after the command that filled it has returned.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len()) };
    Ok(bytes.len() as u32)
}

#[cfg(windows)]
pub fn create_shared(
    webview: &tauri::Webview,
    sizes: Vec<usize>,
) -> Result<Vec<(usize, usize)>, String> {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    use windows::core::{HSTRING, Interface, PCWSTR};
    let (tx, rx) = std::sync::mpsc::channel();
    webview
        .with_webview(move |wv| {
            let r = (|| -> windows::core::Result<Vec<(usize, usize)>> {
                unsafe {
                    let env: ICoreWebView2Environment12 = wv.environment().cast()?;
                    let core: ICoreWebView2_17 = wv.controller().CoreWebView2()?.cast()?;
                    let mut out = vec![];
                    for (i, &size) in sizes.iter().enumerate() {
                        let buf = env.CreateSharedBuffer(size as u64)?;
                        let mut p: *mut u8 = std::ptr::null_mut();
                        buf.Buffer(&mut p)?;
                        let json = HSTRING::from(format!("{{\"slot\":{i}}}"));
                        core.PostSharedBufferToScript(
                            &buf,
                            COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY,
                            PCWSTR::from_raw(json.as_ptr()),
                        )?;
                        out.push((p as usize, size));
                        // keep the mapping alive for the lifetime of the process (spike)
                        std::mem::forget(buf);
                    }
                    Ok(out)
                }
            })();
            let _ = tx.send(r.map_err(|e| e.to_string()));
        })
        .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())?
}

#[cfg(not(windows))]
pub fn create_shared(
    _webview: &tauri::Webview,
    _sizes: Vec<usize>,
) -> Result<Vec<(usize, usize)>, String> {
    Err("shared buffers are a WebView2 (Windows) feature".into())
}
