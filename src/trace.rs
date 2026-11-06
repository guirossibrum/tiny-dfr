//! Small vector tracing animation. No threads, external processes, or frame assets.
use anyhow::{ensure, Result};
use cairo::{Context, LineCap, LineJoin};
use serde::Deserialize;
use std::time::Instant;
use ttfx::{
    effects::wipe::WipeConfig,
    engine::{
        ctx::Clock,
        terminal::{CharacterGroup, TerminalConfig},
    },
    fx::{effects::wipe::Wipe, run::Effect, Engine},
    utils::{
        easing::Easing,
        graphics::{Color, GradientDirection},
        rng::Rng,
    },
};

const SAMPLES: usize = 128;

/// Run the real ttfx Wipe effect once against a one-row canvas. Only its
/// coverage masks survive: no terminal output, threads, or engine per frame.
fn wipe_frames() -> Result<Vec<[bool; SAMPLES]>> {
    let terminal = TerminalConfig {
        canvas_width: SAMPLES as i64,
        canvas_height: 1,
        ignore_terminal_dimensions: true,
        ..TerminalConfig::default()
    };
    let mut engine = Engine::new(
        &"#".repeat(SAMPLES),
        terminal,
        Rng::seeded(0),
        Clock::virtual_with_frame_rate(30),
    )?;
    let mut effect = Wipe::new(WipeConfig {
        wipe_direction: CharacterGroup::ColumnLeftToRight,
        wipe_delay: 0,
        wipe_ease: Easing::InOutCirc,
        final_gradient_stops: vec![Color::from_hex("ffffff").map_err(anyhow::Error::msg)?],
        final_gradient_steps: vec![1],
        final_gradient_frames: 1,
        final_gradient_direction: GradientDirection::Horizontal,
    });
    effect.build(&mut engine)?;
    let mut frames = vec![[false; SAMPLES]];
    while effect.next_frame(&mut engine) {
        let mut frame = [false; SAMPLES];
        for &slot in &engine.input_chars {
            let column = engine.input_coord(slot).column as usize - 1;
            frame[column] = engine.is_visible(slot);
        }
        frames.push(frame);
        engine.clock.advance_frame();
        ensure!(frames.len() < 1024, "ttfx wipe did not finish");
    }
    ensure!(
        frames
            .last()
            .is_some_and(|frame| frame.iter().all(|&on| on)),
        "ttfx wipe did not cover the entire path"
    );
    Ok(frames)
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct TraceConfig {
    /// Disconnected polylines in normalized icon coordinates (0..1).
    pub paths: Vec<Vec<[f64; 2]>>,
    #[serde(default = "green")]
    pub color: [f64; 3],
    #[serde(default = "gray")]
    pub base_color: [f64; 3],
    #[serde(default = "line_width")]
    pub line_width: f64,
    #[serde(default = "duration")]
    pub duration_seconds: f64,
    #[serde(default = "pause")]
    pub pause_seconds: f64,
}

fn green() -> [f64; 3] {
    [0.57, 0.80, 0.33]
}
fn gray() -> [f64; 3] {
    [0.40, 0.40, 0.40]
}
fn line_width() -> f64 {
    0.068
}
fn duration() -> f64 {
    3.2
}
fn pause() -> f64 {
    0.5
}

struct Segment {
    from: [f64; 2],
    to: [f64; 2],
    offset: f64,
    length: f64,
}

pub struct Trace {
    config: TraceConfig,
    segments: Vec<Segment>,
    length: f64,
    started: Instant,
    origins: [usize; 64],
    frames: Vec<[bool; SAMPLES]>,
}

impl Trace {
    pub fn new(config: TraceConfig) -> Result<Self> {
        Self::with_seed(config, rand::random())
    }

    pub fn with_seed(config: TraceConfig, seed: u64) -> Result<Self> {
        ensure!(
            !config.paths.is_empty() && config.paths.len() <= 64,
            "Trace requires 1..64 paths"
        );
        ensure!(
            config.paths.iter().map(Vec::len).sum::<usize>() <= 1024,
            "Too many trace points"
        );
        ensure!(
            config.duration_seconds.is_finite() && (0.5..=120.0).contains(&config.duration_seconds),
            "Invalid trace duration"
        );
        ensure!(
            config.pause_seconds.is_finite() && (0.0..=60.0).contains(&config.pause_seconds),
            "Invalid trace pause"
        );
        ensure!(
            config.line_width.is_finite() && (0.001..=0.25).contains(&config.line_width),
            "Invalid trace width"
        );
        for value in config
            .color
            .iter()
            .chain(config.base_color.iter())
            .chain(config.paths.iter().flatten().flatten())
        {
            ensure!(
                value.is_finite() && (0.0..=1.0).contains(value),
                "Trace coordinates/colors must be in 0..1"
            );
        }
        let mut segments = Vec::new();
        let mut length = 0.0;
        for path in &config.paths {
            ensure!(path.len() >= 2, "Trace paths require at least two points");
            for pair in path.windows(2) {
                let distance = (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
                if distance > f64::EPSILON {
                    segments.push(Segment {
                        from: pair[0],
                        to: pair[1],
                        offset: length,
                        length: distance,
                    });
                    length += distance;
                }
            }
        }
        ensure!(length > 0.0, "Trace paths must have positive length");
        let mut rng = Rng::seeded(seed);
        let mut origins = [0; 64];
        for i in 0..origins.len() {
            loop {
                let candidate = rng.choice_index(SAMPLES);
                if (i == 0 || candidate != origins[i - 1])
                    && (i != origins.len() - 1 || candidate != origins[0])
                {
                    origins[i] = candidate;
                    break;
                }
            }
        }
        let frames = wipe_frames()?;
        Ok(Self {
            config,
            segments,
            length,
            started: Instant::now(),
            origins,
            frames,
        })
    }

    // Random offsets use ttfx's own RNG, computed only when loading the icon.
    fn origin(&self, phase: u64) -> f64 {
        self.origins[phase as usize % self.origins.len()] as f64 / SAMPLES as f64 * self.length
    }

    fn stroke_range(&self, c: &Context, start: f64, end: f64) {
        c.new_path();
        for segment in &self.segments {
            let low = start.max(segment.offset);
            let high = end.min(segment.offset + segment.length);
            if high <= low {
                continue;
            }
            let point = |distance: f64| {
                let t = (distance - segment.offset) / segment.length;
                [
                    segment.from[0] + (segment.to[0] - segment.from[0]) * t,
                    segment.from[1] + (segment.to[1] - segment.from[1]) * t,
                ]
            };
            let from = point(low);
            let to = point(high);
            c.move_to(from[0], from[1]);
            c.line_to(to[0], to[1]);
        }
        c.stroke().unwrap();
    }

    pub fn render(&self, c: &Context, x: f64, y: f64, width: f64, height: f64) {
        self.render_at(c, x, y, width, height, self.started.elapsed().as_secs_f64());
    }

    pub fn render_at(&self, c: &Context, x: f64, y: f64, width: f64, height: f64, elapsed: f64) {
        let cycle = self.config.duration_seconds + self.config.pause_seconds;
        let phase = (elapsed / cycle).floor() as u64;
        let progress = ((elapsed % cycle) / self.config.duration_seconds).clamp(0.0, 1.0);
        // Timing and reveal order come from the existing ttfx Wipe effect.
        let frame = &self.frames[((self.frames.len() - 1) as f64 * progress).round() as usize];
        let (base, fill) = if phase % 2 == 0 {
            (self.config.base_color, self.config.color)
        } else {
            (self.config.color, self.config.base_color)
        };
        c.save().unwrap();
        c.rectangle(x, y, width, height);
        c.clip();
        c.translate(x, y);
        c.scale(width, height);
        c.set_line_width(self.config.line_width);
        c.set_line_cap(LineCap::Square);
        c.set_line_join(LineJoin::Miter);
        let background = if progress >= 1.0 - 1e-9 { fill } else { base };
        c.set_source_rgb(background[0], background[1], background[2]);
        self.stroke_range(c, 0.0, self.length);
        c.set_source_rgb(fill[0], fill[1], fill[2]);
        if progress > 1e-9 && progress < 1.0 - 1e-9 {
            let origin = self.origin(phase);
            // Merge adjacent samples before stroking; only the adapter maps
            // ttfx's one-row coverage onto the configured vector path.
            let mut first = 0;
            while first < SAMPLES {
                if !frame[first] {
                    first += 1;
                    continue;
                }
                let mut last = first + 1;
                while last < SAMPLES && frame[last] {
                    last += 1;
                }
                let start = (origin + first as f64 / SAMPLES as f64 * self.length) % self.length;
                let end = start + (last - first) as f64 / SAMPLES as f64 * self.length;
                self.stroke_range(c, start, end.min(self.length));
                if end > self.length {
                    self.stroke_range(c, 0.0, end - self.length);
                }
                first = last;
            }
        }
        c.restore().unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairo::{Format, ImageSurface};

    fn config() -> TraceConfig {
        TraceConfig {
            paths: vec![vec![[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]]],
            color: green(),
            base_color: gray(),
            line_width: line_width(),
            duration_seconds: duration(),
            pause_seconds: pause(),
        }
    }
    fn pixels(trace: &Trace, time: f64) -> Vec<u8> {
        let mut surface = ImageSurface::create(Format::ARgb32, 64, 64).unwrap();
        {
            let c = Context::new(&surface).unwrap();
            trace.render_at(&c, 0.0, 0.0, 64.0, 64.0, time);
        }
        let pixels = surface.data().unwrap().to_vec();
        pixels
    }
    #[test]
    fn colors_reverse_without_a_flash_at_phase_boundaries() {
        let trace = Trace::with_seed(config(), 42).unwrap();
        assert!(pixels(&trace, 3.2) == pixels(&trace, 3.7));
        assert!(pixels(&trace, 6.9) == pixels(&trace, 7.4));
        assert_ne!(pixels(&trace, 0.0), pixels(&trace, 1.6));
        assert_ne!(pixels(&trace, 1.6), pixels(&trace, 3.2));
    }
    #[test]
    fn origins_change_and_stay_on_the_path() {
        let trace = Trace::with_seed(config(), 42).unwrap();
        for phase in 0..100 {
            assert!((0.0..trace.length).contains(&trace.origin(phase)));
            assert_ne!(trace.origin(phase), trace.origin(phase + 1));
        }
    }
    #[test]
    fn rejects_degenerate_or_unbounded_paths() {
        let mut bad = config();
        bad.paths = vec![vec![[0.5, 0.5], [0.5, 0.5]]];
        assert!(Trace::new(bad).is_err());
        let mut bad = config();
        bad.paths[0][0][0] = f64::NAN;
        assert!(Trace::new(bad).is_err());
        let mut bad = config();
        bad.duration_seconds = 0.0;
        assert!(Trace::new(bad).is_err());
    }
}
