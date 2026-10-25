//! Disk cache for pyramids: one pack file per level (LZ4-compressed raw tiles incl. gutter, index in
//! a footer), the base texture and a `meta.json` written last as the completeness marker.
//! Location: OS cache dir / pano-retouch-spike-s1 / <hash of path, size, mtime>.

use crate::pyramid::{Base, LevelInfo, Pyramid, SLOT, Storage};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, String>;

const MAGIC: &[u8; 8] = b"PRSPK001";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Meta {
    pub width: u32,
    pub height: u32,
    pub storage: Storage,
    pub levels: Vec<LevelInfo>,
    pub base_mips: Vec<(u32, u32)>,
}

pub fn root() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("pano-retouch-spike-s1")
}

pub fn dir_for(source: &Path) -> Result<PathBuf> {
    if crate::decode::kind_of(source) == Some(crate::decode::Kind::Synthetic) {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        source.hash(&mut h);
        return Ok(root().join(format!("{:016x}", h.finish())));
    }
    let md = fs::metadata(source).map_err(|e| e.to_string())?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    fs::canonicalize(source)
        .map_err(|e| e.to_string())?
        .hash(&mut h);
    md.len().hash(&mut h);
    md.modified().ok().hash(&mut h);
    Ok(root().join(format!("{:016x}", h.finish())))
}

pub fn clear_all() -> Result<()> {
    let r = root();
    if r.exists() {
        fs::remove_dir_all(&r).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Writes the pyramid tile row by tile row (compression in parallel, writing sequentially) to keep
/// peak memory low.
pub fn write(dir: &Path, pyr: &Pyramid, base: &Base, levels: &[LevelInfo]) -> Result<u64> {
    let tmp = dir.with_extension("tmp");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let mut total = 0u64;
    for (li, info) in levels.iter().enumerate() {
        let f = File::create(tmp.join(format!("L{li}.pack"))).map_err(|e| e.to_string())?;
        let mut w = BufWriter::with_capacity(8 << 20, f);
        let mut index: Vec<(u64, u32)> = Vec::with_capacity((info.tiles_x * info.tiles_y) as usize);
        let mut off = 0u64;
        for ty in 0..info.tiles_y {
            let row: Vec<Vec<u8>> = (0..info.tiles_x)
                .into_par_iter()
                .map(|tx| {
                    lz4_flex::compress_prepend_size(&pyr.equirect_tile_raw(li, tx, ty).unwrap())
                })
                .collect();
            for t in row {
                w.write_all(&t).map_err(|e| e.to_string())?;
                index.push((off, t.len() as u32));
                off += t.len() as u64;
            }
        }
        for (o, l) in &index {
            w.write_all(&o.to_le_bytes()).map_err(|e| e.to_string())?;
            w.write_all(&l.to_le_bytes()).map_err(|e| e.to_string())?;
        }
        w.write_all(&(index.len() as u32).to_le_bytes())
            .map_err(|e| e.to_string())?;
        w.write_all(MAGIC).map_err(|e| e.to_string())?;
        w.flush().map_err(|e| e.to_string())?;
        total += off;
    }
    fs::write(
        tmp.join("base.lz4"),
        lz4_flex::compress_prepend_size(&base.raw),
    )
    .map_err(|e| e.to_string())?;
    let meta = Meta {
        width: levels[0].w,
        height: levels[0].h,
        storage: pyr.storage(),
        levels: levels.to_vec(),
        base_mips: base.mips.clone(),
    };
    fs::write(tmp.join("meta.json"), serde_json::to_vec(&meta).unwrap())
        .map_err(|e| e.to_string())?;
    let _ = fs::remove_dir_all(dir);
    fs::rename(&tmp, dir).map_err(|e| e.to_string())?;
    Ok(total)
}

pub struct Reader {
    pub meta: Meta,
    packs: Vec<(File, Vec<(u64, u32)>)>,
}

impl Reader {
    pub fn open(dir: &Path) -> Result<Option<(Reader, Base)>> {
        let Ok(meta_bytes) = fs::read(dir.join("meta.json")) else {
            return Ok(None);
        };
        let meta: Meta = serde_json::from_slice(&meta_bytes).map_err(|e| e.to_string())?;
        let mut packs = vec![];
        for li in 0..meta.levels.len() {
            let f = File::open(dir.join(format!("L{li}.pack"))).map_err(|e| e.to_string())?;
            let len = f.metadata().map_err(|e| e.to_string())?.len();
            let mut tail = [0u8; 12];
            read_at(&f, &mut tail, len - 12)?;
            if &tail[4..] != MAGIC {
                return Err("bad pack".into());
            }
            let n = u32::from_le_bytes(tail[..4].try_into().unwrap()) as u64;
            let mut idx = vec![0u8; (n * 12) as usize];
            read_at(&f, &mut idx, len - 12 - n * 12)?;
            let index = idx
                .chunks_exact(12)
                .map(|c| {
                    (
                        u64::from_le_bytes(c[..8].try_into().unwrap()),
                        u32::from_le_bytes(c[8..].try_into().unwrap()),
                    )
                })
                .collect();
            packs.push((f, index));
        }
        let base_raw = lz4_flex::decompress_size_prepended(
            &fs::read(dir.join("base.lz4")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let base = Base {
            storage: meta.storage,
            mips: meta.base_mips.clone(),
            raw: base_raw,
        };
        Ok(Some((Reader { meta, packs }, base)))
    }

    pub fn tile_raw(&self, level: usize, tx: u32, ty: u32) -> Result<Vec<u8>> {
        let (f, index) = self.packs.get(level).ok_or("bad level")?;
        let info = &self.meta.levels[level];
        if tx >= info.tiles_x || ty >= info.tiles_y {
            return Err("tile out of range".into());
        }
        let (off, len) = index[(ty * info.tiles_x + tx) as usize];
        let mut buf = vec![0u8; len as usize];
        read_at(f, &mut buf, off)?;
        let raw = lz4_flex::decompress_size_prepended(&buf).map_err(|e| e.to_string())?;
        debug_assert_eq!(
            raw.len(),
            (SLOT * SLOT * 4) as usize * self.meta.storage.bytes_per_sample()
        );
        Ok(raw)
    }

    /// Reassembles full levels from the cache (needed for on-demand cube tiles).
    pub fn to_pyramid(&self) -> Result<Pyramid> {
        use crate::pyramid::{GUTTER, Level, Pixels, TILE};
        let bps = self.meta.storage.bytes_per_sample();
        let mut levels = vec![];
        for (li, info) in self.meta.levels.iter().enumerate() {
            let (w, h) = (info.w as usize, info.h as usize);
            let row_bytes = w * 4 * bps;
            let mut bytes = vec![0u8; row_bytes * h];
            bytes
                .par_chunks_mut(row_bytes * TILE as usize)
                .enumerate()
                .try_for_each(|(ty, band)| -> Result<()> {
                    for tx in 0..info.tiles_x {
                        let raw = self.tile_raw(li, tx, ty as u32)?;
                        let x0 = tx as usize * TILE as usize;
                        let cw = (w - x0).min(TILE as usize);
                        let rows = band.len() / row_bytes;
                        for j in 0..rows {
                            let src =
                                ((j + GUTTER as usize) * SLOT as usize + GUTTER as usize) * 4 * bps;
                            let dst = j * row_bytes + x0 * 4 * bps;
                            band[dst..dst + cw * 4 * bps]
                                .copy_from_slice(&raw[src..src + cw * 4 * bps]);
                        }
                    }
                    Ok(())
                })?;
            levels.push(Level {
                width: info.w,
                height: info.h,
                px: Pixels::from_bytes(self.meta.storage, &bytes),
            });
        }
        Ok(Pyramid {
            levels: levels.into_iter().map(parking_lot::RwLock::new).collect(),
        })
    }
}

#[cfg(unix)]
fn read_at(f: &File, buf: &mut [u8], off: u64) -> Result<()> {
    use std::os::unix::fs::FileExt;
    f.read_exact_at(buf, off).map_err(|e| e.to_string())
}

#[cfg(windows)]
fn read_at(f: &File, buf: &mut [u8], mut off: u64) -> Result<()> {
    use std::os::windows::fs::FileExt;
    let mut done = 0;
    while done < buf.len() {
        let n = f
            .seek_read(&mut buf[done..], off)
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("unexpected EOF".into());
        }
        done += n;
        off += n as u64;
    }
    Ok(())
}
