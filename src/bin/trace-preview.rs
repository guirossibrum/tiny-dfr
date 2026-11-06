//! Render the exact Touch Bar trace renderer without DRM or root privileges.
#[allow(dead_code)]
#[path = "../trace.rs"]
mod trace;

use anyhow::{Context as _, Result};
use cairo::{Context, Format, ImageSurface};
use serde::Deserialize;
use std::{fs, path::PathBuf};

#[derive(Deserialize)]
struct Preview {
    #[serde(rename = "Trace")]
    trace: trace::TraceConfig,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    anyhow::ensure!(
        args.len() == 3,
        "Usage: trace-preview CONFIG.toml OUTPUT_DIRECTORY"
    );
    let config: Preview = toml::from_str(&fs::read_to_string(&args[1])?)?;
    let animation = trace::Trace::with_seed(config.trace, 42)?;
    let output = PathBuf::from(&args[2]);
    fs::create_dir_all(&output)?;
    for frame in 0..222 {
        let surface = ImageSurface::create(Format::ARgb32, 192, 192)?;
        let c = Context::new(&surface)?;
        c.set_source_rgb(0.0, 0.0, 0.0);
        c.paint()?;
        animation.render_at(&c, 4.0, 4.0, 184.0, 184.0, frame as f64 / 15.0);
        let mut file = fs::File::create(output.join(format!("{frame:04}.png")))?;
        surface
            .write_to_png(&mut file)
            .context("write preview frame")?;
    }
    Ok(())
}
