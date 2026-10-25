//! One opened image: probe, fast base preview, full decode → pyramid, disk cache, tile access.

use crate::cache;
use crate::decode::{self, JpegDecoder, Kind};
use crate::pyramid::{self, Base, CubeLevelInfo, LevelInfo, Pyramid, Storage, Wire};
use parking_lot::RwLock;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

#[derive(Serialize, Clone, Debug)]
pub struct OpenInfo {
    pub session: u32,
    pub path: String,
    pub kind: Kind,
    pub width: u32,
    pub height: u32,
    pub storage: Storage,
    pub tile: u32,
    pub slot: u32,
    pub levels: Vec<LevelInfo>,
    pub cube_levels: Vec<CubeLevelInfo>,
    pub base_w: u32,
    pub base_h: u32,
    pub from_cache: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct Status {
    pub session: u32,
    pub stage: &'static str,
    pub ms: f64,
    pub detail: String,
}

#[derive(Default)]
struct Data {
    base: Option<Arc<Base>>,
    pyramid: Option<Arc<Pyramid>>,
    cache: Option<Arc<cache::Reader>>,
    error: Option<String>,
}

pub struct Session {
    pub info: OpenInfo,
    pub start: Instant,
    data: RwLock<Data>,
    cube_loading: std::sync::Mutex<bool>,
    /// S2: edit version – incremented by every committed stroke
    pub version: std::sync::atomic::AtomicU32,
    commit_lock: std::sync::Mutex<()>,
}

#[derive(Serialize, Clone, Debug)]
pub struct CommitResult {
    pub version: u32,
    /// [level, tx, ty]
    pub dirty: Vec<[u32; 3]>,
    pub pixels: usize,
    pub tiles_tested: usize,
    pub ms_raster: f64,
    pub ms_pyramid: f64,
    pub ms_base: f64,
    pub ms_total: f64,
}

pub enum TileError {
    NotReady,
    Bad(String),
}

pub struct Options {
    pub use_cache: bool,
    pub write_cache: bool,
    pub jpeg: JpegDecoder,
}

impl Session {
    pub fn ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }

    /// Probes the file (fast) and returns immediately; the heavy work runs on background threads and
    /// reports progress through `notify`.
    pub fn open(
        id: u32,
        path: PathBuf,
        opts: Options,
        notify: impl Fn(Status) + Send + Sync + Clone + 'static,
    ) -> Result<Arc<Session>, String> {
        let start = Instant::now();
        let cache_dir = cache::dir_for(&path)?;
        let cached = if opts.use_cache {
            cache::Reader::open(&cache_dir)?
        } else {
            None
        };
        let probe = decode::probe(&path)?;
        let levels = pyramid::level_infos(probe.width, probe.height);
        let info = OpenInfo {
            session: id,
            path: path.to_string_lossy().into(),
            kind: probe.kind,
            width: probe.width,
            height: probe.height,
            storage: probe.storage,
            tile: pyramid::TILE,
            slot: pyramid::SLOT,
            cube_levels: pyramid::cube_level_infos(&levels),
            levels,
            base_w: pyramid::BASE_W,
            base_h: pyramid::BASE_H,
            from_cache: cached.is_some(),
        };
        let s = Arc::new(Session {
            info,
            start,
            data: RwLock::default(),
            cube_loading: Default::default(),
            version: Default::default(),
            commit_lock: Default::default(),
        });

        if let Some((reader, base)) = cached {
            let mut d = s.data.write();
            d.base = Some(Arc::new(base));
            d.cache = Some(Arc::new(reader));
            drop(d);
            let n = notify.clone();
            let ss = s.clone();
            // Emit asynchronously so the frontend has the OpenInfo before the first event.
            std::thread::spawn(move || {
                n(Status {
                    session: id,
                    stage: "base",
                    ms: ss.ms(),
                    detail: "from cache".into(),
                });
                n(Status {
                    session: id,
                    stage: "tiles",
                    ms: ss.ms(),
                    detail: "from cache".into(),
                });
            });
            return Ok(s);
        }

        if probe.kind == Kind::Jpeg {
            let (ss, n, p) = (s.clone(), notify.clone(), path.clone());
            std::thread::spawn(move || match decode::jpeg_quick(&p) {
                Ok(l) => {
                    let t = ss.ms();
                    let base = Base::build(&l);
                    let mut d = ss.data.write();
                    if d.base.is_none() {
                        d.base = Some(Arc::new(base));
                        drop(d);
                        n(Status {
                            session: id,
                            stage: "base",
                            ms: ss.ms(),
                            detail: format!(
                                "DCT 1/8 decode {:.0} ms ({}×{})",
                                t, l.width, l.height
                            ),
                        });
                    }
                }
                Err(e) => eprintln!("quick jpeg failed: {e}"),
            });
        }

        let (ss, n) = (s.clone(), notify);
        std::thread::spawn(move || {
            if let Err(e) = full_pipeline(&ss, &path, &cache_dir, &opts, &n) {
                ss.data.write().error = Some(e.clone());
                n(Status {
                    session: id,
                    stage: "error",
                    ms: ss.ms(),
                    detail: e,
                });
            }
        });
        Ok(s)
    }

    pub fn base_wire(&self, wire: Wire) -> Result<Vec<u8>, TileError> {
        let base = self.data.read().base.clone().ok_or(TileError::NotReady)?;
        Ok(pyramid::to_wire(base.storage, base.raw.clone(), wire))
    }

    pub fn equirect_tile(
        &self,
        level: usize,
        tx: u32,
        ty: u32,
        wire: Wire,
    ) -> Result<Vec<u8>, TileError> {
        let (pyr, cache, storage) = {
            let d = self.data.read();
            (d.pyramid.clone(), d.cache.clone(), self.info.storage)
        };
        let raw = if let Some(p) = pyr {
            return p
                .equirect_tile_wire(level, tx, ty, wire)
                .ok_or_else(|| TileError::Bad("bad tile".into()));
        } else if let Some(c) = cache {
            c.tile_raw(level, tx, ty).map_err(TileError::Bad)?
        } else {
            return Err(TileError::NotReady);
        };
        Ok(pyramid::to_wire(storage, raw, wire))
    }

    /// The in-memory pyramid; a session re-opened from the disk cache reassembles it once (slow, spike only).
    fn pyramid(&self) -> Result<Arc<Pyramid>, TileError> {
        if let Some(p) = self.data.read().pyramid.clone() {
            return Ok(p);
        }
        let cache = self.data.read().cache.clone().ok_or(TileError::NotReady)?;
        let _g = self.cube_loading.lock().unwrap();
        if let Some(p) = self.data.read().pyramid.clone() {
            return Ok(p);
        }
        let t = Instant::now();
        let p = Arc::new(cache.to_pyramid().map_err(TileError::Bad)?);
        eprintln!(
            "reassembled pyramid from cache in {:.0} ms",
            t.elapsed().as_secs_f64() * 1e3
        );
        self.data.write().pyramid = Some(p.clone());
        Ok(p)
    }

    pub fn cube_tile(
        &self,
        face: u32,
        level: usize,
        tx: u32,
        ty: u32,
        wire: Wire,
    ) -> Result<Vec<u8>, TileError> {
        let pyr = self.pyramid()?;
        let n = self
            .info
            .cube_levels
            .get(level)
            .ok_or_else(|| TileError::Bad("bad level".into()))?
            .n;
        let raw = pyr
            .cube_tile_raw(face, level, n, tx, ty)
            .ok_or_else(|| TileError::Bad("bad tile".into()))?;
        Ok(pyramid::to_wire(self.info.storage, raw, wire))
    }

    /// S2: rasterises a stroke at full resolution, updates all levels and the base texture and returns
    /// the tiles that changed. Commits are serialised; tile requests keep being served meanwhile.
    pub fn commit(&self, stroke: &crate::edit::Stroke) -> Result<CommitResult, String> {
        let pyr = self.pyramid().map_err(|e| match e {
            TileError::NotReady => "image not ready".to_string(),
            TileError::Bad(m) => m,
        })?;
        let _g = self.commit_lock.lock().unwrap();
        let t0 = Instant::now();
        let applied = crate::edit::apply(&pyr, stroke);
        let ms_raster = t0.elapsed().as_secs_f64() * 1e3;
        let t1 = Instant::now();
        let rects = crate::edit::propagate(&pyr, applied.rects);
        let ms_pyramid = t1.elapsed().as_secs_f64() * 1e3;
        let t2 = Instant::now();
        let base = Arc::new(Base::build(&pyr.levels.last().unwrap().read()));
        let ms_base = t2.elapsed().as_secs_f64() * 1e3;
        self.data.write().base = Some(base);
        let mut dirty = vec![];
        for (li, r) in rects.iter().enumerate() {
            let info = &self.info.levels[li];
            for (tx, ty) in crate::edit::dirty_tiles(info.w, info.h, r) {
                dirty.push([li as u32, tx, ty]);
            }
        }
        let version = self
            .version
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        Ok(CommitResult {
            version,
            dirty,
            pixels: applied.pixels,
            tiles_tested: applied.tiles_tested,
            ms_raster,
            ms_pyramid,
            ms_base,
            ms_total: t0.elapsed().as_secs_f64() * 1e3,
        })
    }
}

fn full_pipeline(
    s: &Arc<Session>,
    path: &Path,
    cache_dir: &Path,
    opts: &Options,
    notify: &impl Fn(Status),
) -> Result<(), String> {
    let id = s.info.session;
    let t0 = Instant::now();
    let l0 = decode::decode_full(path, s.info.kind, opts.jpeg)?;
    let t_decode = t0.elapsed().as_secs_f64() * 1e3;
    let t1 = Instant::now();
    let pyr = Arc::new(Pyramid::build(l0));
    let t_pyr = t1.elapsed().as_secs_f64() * 1e3;
    let t2 = Instant::now();
    let base = Arc::new(Base::build(&pyr.levels.last().unwrap().read()));
    let t_base = t2.elapsed().as_secs_f64() * 1e3;
    {
        let mut d = s.data.write();
        d.pyramid = Some(pyr.clone());
        let had_base = d.base.is_some();
        if !had_base {
            d.base = Some(base.clone());
        }
        drop(d);
        if !had_base {
            notify(Status {
                session: id,
                stage: "base",
                ms: s.ms(),
                detail: "from pyramid".into(),
            });
        }
    }
    notify(Status {
        session: id,
        stage: "tiles",
        ms: s.ms(),
        detail: format!("decode {t_decode:.0} ms, pyramid {t_pyr:.0} ms, base {t_base:.0} ms"),
    });
    if opts.write_cache {
        let t3 = Instant::now();
        let bytes = cache::write(cache_dir, &pyr, &base, &s.info.levels)?;
        notify(Status {
            session: id,
            stage: "cache-written",
            ms: s.ms(),
            detail: format!(
                "{:.0} MB in {:.0} ms",
                bytes as f64 / 1e6,
                t3.elapsed().as_secs_f64() * 1e3
            ),
        });
    }
    Ok(())
}

pub fn peak_rss_mb() -> f64 {
    #[cfg(unix)]
    unsafe {
        let mut u: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut u);
        // bytes on macOS, KiB on Linux
        let v = u.ru_maxrss as f64;
        if cfg!(target_os = "macos") {
            v / 1e6
        } else {
            v * 1024.0 / 1e6
        }
    }
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        if GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) != 0 {
            c.PeakWorkingSetSize as f64 / 1e6
        } else {
            0.0
        }
    }
}
