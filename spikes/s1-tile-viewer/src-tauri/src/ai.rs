//! Spike S3 – ONNX inpainting models via ONNX Runtime (`ort`).
//!
//! - LaMa (Big-LaMa export by Carve, `lama_fp32.onnx`): fixed 512×512, inputs `image` [1,3,512,512]
//!   f32 0..1 and `mask` [1,1,512,512] f32 (1 = hole); output [1,3,512,512].
//! - MI-GAN (`migan_pipeline_v2.onnx`): arbitrary size, inputs `image` [1,3,H,W] u8 and `mask`
//!   [1,1,H,W] u8 (255 = known, 0 = hole); pre/post-processing (crop, resize to 512, blend) is inside.

use crate::inpaint::Img;
use ort::session::Session;
#[allow(unused_imports)]
use ort::ep::ExecutionProvider;
use ort::value::Tensor;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Cpu,
    CoreMl,
    DirectMl,
}

impl Provider {
    pub fn parse(s: &str) -> Option<Provider> {
        match s {
            "cpu" => Some(Provider::Cpu),
            "coreml" => Some(Provider::CoreMl),
            "directml" => Some(Provider::DirectMl),
            _ => None,
        }
    }
}

pub struct Model {
    pub session: Session,
    pub kind: ModelKind,
    pub load_ms: f64,
    pub provider: Provider,
    /// which execution provider actually runs the model ("coreml-mlprogram", "cpu", …)
    pub used: String,
    pub fallback_reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelKind {
    Lama,
    Migan,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn environment() -> Result<&'static ort::environment::Environment, String> {
    static ENV: std::sync::OnceLock<ort::environment::Environment> = std::sync::OnceLock::new();
    if let Some(e) = ENV.get() {
        return Ok(e);
    }
    let e = ort::init().with_name("pano-retouch-s3").build().map_err(err)?;
    Ok(ENV.get_or_init(|| e))
}

/// Loads a model. CoreML falls back from MLProgram to NeuralNetwork to CPU; `used` tells which one.
pub fn load(path: &std::path::Path, kind: ModelKind, provider: Provider) -> Result<Model, String> {
    let env = environment()?;
    let attempts: Vec<&str> = match provider {
        Provider::Cpu => vec!["cpu"],
        Provider::CoreMl => vec!["coreml-mlprogram", "coreml-neuralnetwork", "cpu"],
        Provider::DirectMl => vec!["directml", "cpu"],
    };
    let mut last = String::new();
    for a in attempts {
        let t = Instant::now();
        let r = (|| -> Result<Session, String> {
            let mut b = Session::builder(env).map_err(err)?;
            // S3_OPT=disable|1|2|all (default all): graph optimisation level – affects load time and memory
            use ort::session::builder::GraphOptimizationLevel as L;
            let lvl = match std::env::var("S3_OPT").as_deref() {
                Ok("disable") => L::Disable,
                Ok("1") => L::Level1,
                Ok("2") => L::Level2,
                _ => L::Level3,
            };
            b = b.with_optimization_level(lvl).map_err(err)?;
            #[allow(unused_mut)]
            let mut eps = vec![];
            match a {
                #[cfg(target_os = "macos")]
                "coreml-mlprogram" | "coreml-neuralnetwork" => {
                    use ort::ep::coreml::{ComputeUnits, ModelFormat};
                    let cache = std::env::temp_dir().join(format!("pano-retouch-coreml-cache-{a}"));
                    let _ = std::fs::create_dir_all(&cache);
                    let fmt = if a == "coreml-mlprogram" { ModelFormat::MLProgram } else { ModelFormat::NeuralNetwork };
                    eps.push(
                        ort::ep::CoreML::default()
                            .with_model_format(fmt)
                            .with_compute_units(ComputeUnits::All)
                            .with_model_cache_dir(cache.to_string_lossy())
                            .build()
                            .error_on_failure(),
                    );
                }
                // DirectML requires sequential execution and no memory-pattern optimisation
                #[cfg(windows)]
                "directml" => {
                    b = b.with_memory_pattern(false).map_err(err)?.with_parallel_execution(false).map_err(err)?;
                    eps.push(ort::ep::DirectML::default().build().error_on_failure());
                }
                // ort's default device policy registers CoreML on macOS even without asking – opt out
                "cpu" => b = b.with_auto_device(ort::session::builder::AutoDevicePolicy::PreferCPU).map_err(err)?,
                other => return Err(format!("{other} not available on this platform")),
            }
            if !eps.is_empty() {
                b = b.with_execution_providers(eps).map_err(err)?;
            }
            b.commit_from_file(path).map_err(err)
        })();
        match r {
            Ok(session) => {
                return Ok(Model { session, kind, load_ms: t.elapsed().as_secs_f64() * 1e3, provider, used: a.to_string(), fallback_reason: last });
            }
            Err(e) => {
                eprintln!("  {a} failed: {}", e.lines().next().unwrap_or(""));
                last = format!("{a}: {}", e.lines().next().unwrap_or(""));
            }
        }
    }
    Err(last)
}

impl Model {
    pub fn describe(&self) -> String {
        let io = |v: &[ort::value::Outlet]| v.iter().map(|o| format!("{}: {:?}", o.name(), o.dtype())).collect::<Vec<_>>().join(", ");
        format!("inputs [{}] outputs [{}]", io(self.session.inputs()), io(self.session.outputs()))
    }

    /// Fills `hole` of `img` (RGB 0..1, size `img.w`×`img.h`). LaMa needs exactly 512×512.
    pub fn run(&mut self, img: &Img, hole: &[bool]) -> Result<(Img, f64), String> {
        let (w, h) = (img.w, img.h);
        let t = Instant::now();
        let out = match self.kind {
            ModelKind::Lama => {
                if w != 512 || h != 512 {
                    return Err("LaMa export is fixed to 512×512".into());
                }
                let mut im = vec![0f32; 3 * w * h];
                let mut mk = vec![0f32; w * h];
                for p in 0..w * h {
                    for c in 0..3 {
                        im[c * w * h + p] = img.px[p][c].clamp(0.0, 1.0);
                    }
                    mk[p] = if hole[p] { 1.0 } else { 0.0 };
                }
                let image = Tensor::from_array(([1usize, 3, h, w], im)).map_err(err)?;
                let mask = Tensor::from_array(([1usize, 1, h, w], mk)).map_err(err)?;
                let outputs = self.session.run(ort::inputs!["image" => image, "mask" => mask]).map_err(err)?;
                let (_, data) = outputs[0].try_extract_tensor::<f32>().map_err(err)?;
                // this export returns 0..255
                let scale = if data.iter().take(10_000).any(|&v| v > 2.0) { 1.0 / 255.0 } else { 1.0 };
                let mut o = Img::new(w, h);
                for p in 0..w * h {
                    o.px[p] = [data[p] * scale, data[w * h + p] * scale, data[2 * w * h + p] * scale, 1.0];
                }
                o
            }
            ModelKind::Migan => {
                let mut im = vec![0u8; 3 * w * h];
                let mut mk = vec![0u8; w * h];
                for p in 0..w * h {
                    for c in 0..3 {
                        im[c * w * h + p] = (img.px[p][c].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                    }
                    mk[p] = if hole[p] { 0 } else { 255 };
                }
                let image = Tensor::from_array(([1usize, 3, h, w], im)).map_err(err)?;
                let mask = Tensor::from_array(([1usize, 1, h, w], mk)).map_err(err)?;
                let outputs = self.session.run(ort::inputs!["image" => image, "mask" => mask]).map_err(err)?;
                let (_, data) = outputs[0].try_extract_tensor::<u8>().map_err(err)?;
                let mut o = Img::new(w, h);
                for p in 0..w * h {
                    o.px[p] = [data[p] as f32 / 255.0, data[w * h + p] as f32 / 255.0, data[2 * w * h + p] as f32 / 255.0, 1.0];
                }
                o
            }
        };
        Ok((out, t.elapsed().as_secs_f64() * 1e3))
    }
}
