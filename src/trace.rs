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

#[path = "trace_graph.rs"]
mod graph;

const SAMPLES: usize = 128;

/// Run the real ttfx Wipe effect once against a one-row canvas. Only its
/// coverage masks survive: no terminal output, threads, or engine per frame.
fn wipe_frames() -> Result<Vec<f64>> {
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
        wipe_ease: Easing::Linear,
        final_gradient_stops: vec![Color::from_hex("ffffff").map_err(anyhow::Error::msg)?],
        final_gradient_steps: vec![1],
        final_gradient_frames: 1,
        final_gradient_direction: GradientDirection::Horizontal,
    });
    effect.build(&mut engine)?;
    let mut frames = vec![0.0];
    while effect.next_frame(&mut engine) {
        let mut frame = [false; SAMPLES];
        for &slot in &engine.input_chars {
            let column = engine.input_coord(slot).column as usize - 1;
            frame[column] = engine.is_visible(slot);
        }
        frames.push(frame.iter().filter(|&&on| on).count() as f64 / SAMPLES as f64);
        engine.clock.advance_frame();
        ensure!(frames.len() < 1024, "ttfx wipe did not finish");
    }
    ensure!(
        frames.last().is_some_and(|&coverage| coverage == 1.0),
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
    1.0
}

pub struct Trace {
    config: TraceConfig,
    graph: graph::Graph,
    waves: Vec<graph::Wave>,
    durations: Vec<f64>,
    period: f64,
    speed: f64,
    started: Instant,
    frames: Vec<f64>,
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
        let graph = graph::Graph::new(&config.paths)?;
        let mut rng = Rng::seeded(seed);
        let sources = graph.sources(&mut rng, config.line_width)?;
        let waves: Vec<_> = sources
            .into_iter()
            .map(|source| graph.wave(source))
            .collect();
        // One fixed physical speed for every starting point and branch. A pass
        // finishes when its last front arrives, rather than accelerating short rails.
        let speed = graph.diameter / config.duration_seconds;
        let durations: Vec<_> = waves
            .iter()
            .enumerate()
            .map(|(phase, wave)| {
                wave.reach / speed
                    + if phase % 2 == 0 {
                        config.pause_seconds
                    } else {
                        0.0
                    }
            })
            .collect();
        let period = durations.iter().sum();
        let frames = wipe_frames()?;
        Ok(Self {
            config,
            graph,
            waves,
            durations,
            period,
            speed,
            started: Instant::now(),
            frames,
        })
    }

    fn phase_at(&self, elapsed: f64) -> (usize, f64) {
        let mut elapsed = elapsed.rem_euclid(self.period);
        for (index, &duration) in self.durations.iter().enumerate() {
            if elapsed < duration {
                return (index, elapsed);
            }
            elapsed -= duration;
        }
        (0, 0.0)
    }

    fn traveled(&self, phase: usize, elapsed: f64) -> f64 {
        let reach = self.waves[phase].reach;
        let progress = (elapsed * self.speed / reach).clamp(0.0, 1.0);
        // Interpolate the real ttfx Linear Wipe coverage masks so the adapter
        // has no circular easing or coarse per-sample jumps.
        let frame = progress * (self.frames.len() - 1) as f64;
        let low = frame.floor() as usize;
        let high = (low + 1).min(self.frames.len() - 1);
        let t = frame - low as f64;
        (self.frames[low] + (self.frames[high] - self.frames[low]) * t) * reach
    }

    fn stroke(&self, c: &Context, index: usize, start: f64, end: f64) {
        let edge = &self.graph.edges[index];
        let (a, b) = (self.graph.nodes[edge.a], self.graph.nodes[edge.b]);
        let point = |distance: f64| {
            let t = distance / edge.length;
            [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
        };
        let (a, b) = (point(start), point(end));
        c.move_to(a[0], a[1]);
        c.line_to(b[0], b[1]);
    }

    pub fn render(&self, c: &Context, x: f64, y: f64, width: f64, height: f64) {
        self.render_at(c, x, y, width, height, self.started.elapsed().as_secs_f64());
    }

    pub fn render_at(&self, c: &Context, x: f64, y: f64, width: f64, height: f64, elapsed: f64) {
        let (phase, elapsed) = self.phase_at(elapsed);
        let wave = &self.waves[phase];
        let traveled = self.traveled(phase, elapsed);
        let complete = traveled >= wave.reach - 1e-9;
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
        let background = if complete { fill } else { base };
        c.set_source_rgb(background[0], background[1], background[2]);
        c.new_path();
        for (index, edge) in self.graph.edges.iter().enumerate() {
            self.stroke(c, index, 0.0, edge.length);
        }
        c.stroke().unwrap();
        if !complete && traveled > 0.0 {
            c.set_source_rgb(fill[0], fill[1], fill[2]);
            c.new_path();
            for index in 0..self.graph.edges.len() {
                let (spans, count) = self.graph.spans(wave, index, traveled);
                for &(start, end) in &spans[..count] {
                    self.stroke(c, index, start, end);
                }
            }
            c.stroke().unwrap();
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
        let first = trace.durations[0];
        let second = first + trace.durations[1];
        assert!(pixels(&trace, first - 0.1) == pixels(&trace, first));
        assert!(pixels(&trace, second) == pixels(&trace, 0.0));
        assert!(pixels(&trace, 0.0) != pixels(&trace, first / 2.0));
    }
    #[test]
    fn holds_green_for_one_second_and_recolors_gray_immediately() {
        let trace = Trace::with_seed(config(), 42).unwrap();
        let green_finished = trace.waves[0].reach / trace.speed;
        assert!((trace.durations[0] - green_finished - 1.0).abs() < 1e-9);
        assert!(pixels(&trace, green_finished) == pixels(&trace, green_finished + 0.9));
        let gray_started = trace.durations[0];
        let gray_finished = gray_started + trace.durations[1];
        assert!((trace.durations[1] - trace.waves[1].reach / trace.speed).abs() < 1e-9);
        assert!(pixels(&trace, gray_finished) != pixels(&trace, gray_finished + 0.2));
        for (phase, wave) in trace.waves.iter().enumerate() {
            let hold = trace.durations[phase] - wave.reach / trace.speed;
            let expected = if phase % 2 == 0 { 1.0 } else { 0.0 };
            assert!((hold - expected).abs() < 1e-9);
        }
    }
    #[test]
    fn linear_front_speed_does_not_accelerate() {
        let trace = Trace::with_seed(config(), 42).unwrap();
        for phase in 0..trace.waves.len() {
            let travel_time = trace.waves[phase].reach / trace.speed;
            for step in 1..10 {
                let elapsed = travel_time * step as f64 / 10.0;
                let actual = trace.traveled(phase, elapsed);
                let expected = elapsed * trace.speed;
                assert!((actual - expected).abs() < trace.waves[phase].reach * 0.02);
            }
        }
    }
    #[test]
    fn origins_change_and_stay_strictly_inside_rails() {
        let trace = Trace::with_seed(config(), 42).unwrap();
        for (i, wave) in trace.waves.iter().enumerate() {
            let edge = &trace.graph.edges[wave.source.edge];
            assert!(wave.source.offset >= trace.config.line_width);
            assert!(edge.length - wave.source.offset >= trace.config.line_width - 1e-9);
            let next = &trace.waves[(i + 1) % trace.waves.len()];
            assert!(
                wave.source.edge != next.source.edge || wave.source.offset != next.source.offset
            );
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
