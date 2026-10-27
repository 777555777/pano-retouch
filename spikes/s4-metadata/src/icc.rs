//! Source profile → sRGB with Little CMS (`lcms2`) or the pure-Rust `moxcms`, 16-bit, in place,
//! relative colorimetric without black point compensation.

use rayon::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cms {
    Lcms,
    Mox,
}

const CHUNK: usize = 1 << 20; // pixels per parallel job

pub fn description(icc: &[u8]) -> String {
    lcms2::Profile::new_icc(icc)
        .ok()
        .and_then(|p| p.info(lcms2::InfoType::Description, lcms2::Locale::none()))
        .unwrap_or_else(|| "?".into())
}

/// The sRGB profile written into converted files (Little CMS's built-in sRGB).
pub fn srgb_profile() -> Vec<u8> {
    lcms2::Profile::new_srgb().icc().expect("srgb icc")
}

pub fn to_srgb(px: &mut [u16], ch: usize, icc: &[u8], cms: Cms) -> Result<(), String> {
    match cms {
        Cms::Lcms => {
            let ctx = lcms2::ThreadContext::new();
            let src = lcms2::Profile::new_icc_context(&ctx, icc).map_err(|e| e.to_string())?;
            let dst = lcms2::Profile::new_srgb_context(&ctx);
            let fmt = if ch == 4 {
                lcms2::PixelFormat::RGBA_16
            } else {
                lcms2::PixelFormat::RGB_16
            };
            // In place: alpha is left untouched. S4_LCMS_NOOPT=1 disables the 16-bit optimisations.
            let flags = if std::env::var_os("S4_LCMS_NOOPT").is_some() {
                lcms2::Flags::NO_CACHE | lcms2::Flags::NO_OPTIMIZE
            } else {
                lcms2::Flags::NO_CACHE
            };
            if ch == 4 {
                let t = lcms2::Transform::<
                    [u16; 4],
                    [u16; 4],
                    lcms2::ThreadContext,
                    lcms2::DisallowCache,
                >::new_flags_context(
                    &ctx,
                    &src,
                    fmt,
                    &dst,
                    fmt,
                    lcms2::Intent::RelativeColorimetric,
                    flags,
                )
                .map_err(|e| e.to_string())?;
                let v: &mut [[u16; 4]] = bytemuck_cast(px);
                v.par_chunks_mut(CHUNK)
                    .for_each(|c| t.transform_in_place(c));
            } else {
                let t = lcms2::Transform::<
                    [u16; 3],
                    [u16; 3],
                    lcms2::ThreadContext,
                    lcms2::DisallowCache,
                >::new_flags_context(
                    &ctx,
                    &src,
                    fmt,
                    &dst,
                    fmt,
                    lcms2::Intent::RelativeColorimetric,
                    flags,
                )
                .map_err(|e| e.to_string())?;
                let v: &mut [[u16; 3]] = bytemuck_cast(px);
                v.par_chunks_mut(CHUNK)
                    .for_each(|c| t.transform_in_place(c));
            }
        }
        Cms::Mox => {
            let src = moxcms::ColorProfile::new_from_slice(icc).map_err(|e| format!("{e:?}"))?;
            // Same destination profile as the lcms path (and as embedded in the file).
            let dst = moxcms::ColorProfile::new_from_slice(&srgb_profile())
                .map_err(|e| format!("{e:?}"))?;
            let layout = if ch == 4 {
                moxcms::Layout::Rgba
            } else {
                moxcms::Layout::Rgb
            };
            let opts = moxcms::TransformOptions {
                rendering_intent: moxcms::RenderingIntent::RelativeColorimetric,
                // S4_MOX_FLOAT=1: floating point instead of the default Q2.13 fixed point.
                prefer_fixed_point: std::env::var_os("S4_MOX_FLOAT").is_none(),
                ..Default::default()
            };
            let t = src
                .create_transform_16bit(layout, &dst, layout, opts)
                .map_err(|e| format!("{e:?}"))?;
            px.par_chunks_mut(CHUNK * ch).try_for_each(|c| {
                let src = c.to_vec();
                t.transform(&src, c).map_err(|e| format!("{e:?}"))
            })?;
        }
    }
    Ok(())
}

fn bytemuck_cast<const N: usize>(px: &mut [u16]) -> &mut [[u16; N]] {
    assert_eq!(px.len() % N, 0);
    // SAFETY: [u16; N] has the alignment of u16 and no padding.
    unsafe { std::slice::from_raw_parts_mut(px.as_mut_ptr().cast(), px.len() / N) }
}

/// Float reference (Little CMS, double precision, no 16-bit optimisations) for sampled pixels.
pub fn reference(px: &[u16], ch: usize, icc: &[u8]) -> Result<Vec<f64>, String> {
    let src = lcms2::Profile::new_icc(icc).map_err(|e| e.to_string())?;
    let dst = lcms2::Profile::new_srgb();
    let t = lcms2::Transform::<[f64; 3], [f64; 3]>::new_flags(
        &src,
        lcms2::PixelFormat::RGB_DBL,
        &dst,
        lcms2::PixelFormat::RGB_DBL,
        lcms2::Intent::RelativeColorimetric,
        lcms2::Flags::NO_OPTIMIZE,
    )
    .map_err(|e| e.to_string())?;
    let inp: Vec<[f64; 3]> = px
        .chunks_exact(ch)
        .map(|p| [0, 1, 2].map(|c| f64::from(p[c]) / 65535.0))
        .collect();
    let mut out = vec![[0.0; 3]; inp.len()];
    t.transform_pixels(&inp, &mut out);
    Ok(out
        .into_iter()
        .flatten()
        .map(|v| v.clamp(0.0, 1.0) * 65535.0)
        .collect())
}
