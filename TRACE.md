# Vector trace buttons using Omarchy's screensaver engine

This fork adds an optional `Trace` inline table to a button. Keep `Icon` as a
static fallback: unmodified tiny-dfr ignores the new field and shows that icon.
The button's action and Fn behavior do not change.

```toml
{ Icon = "omarchy-green", Action = "Search", Trace = { Paths = [[[0.1, 0.1], [0.9, 0.1], [0.9, 0.9]]], Color = [0.57, 0.80, 0.33], BaseColor = [0.4, 0.4, 0.4], LineWidth = 0.068, DurationSeconds = 3.2, PauseSeconds = 0.5 } }
```

Paths are disconnected polylines in normalized icon coordinates. Width is also
normalized. There may be at most 64 paths and 1024 points; colors and coordinates
must be finite and in 0..1. Duration is 0.5..120 seconds; pause is 0..60 seconds.
Invalid trace settings fall back to the static button with a diagnostic.

The **actual ttfx `fx::effects::wipe::Wipe` implementation** generates coverage
masks on a 128-cell, one-row virtual canvas when the icon loads. It uses ttfx's
InOutCirc easing. The full engine is dropped after these masks are generated;
the per-frame code maps the masks to the vector path and paints with Cairo.
ttfx is pinned to a full Git revision in Cargo.toml and Cargo.lock. No code is
copied out of ttfx, and no terminal, subprocess, or animation thread runs.

Each pass alternates gray-to-green and green-to-gray. ttfx's RNG generates 64
origins when loading; adjacent passes always start at different points, including
the wrap from the last stored origin to the first. The origin sequence repeats
after 64 passes. The effect stays on its current layer and does not reload config
or reset held keys.

Animation is capped at roughly 15 fps. Only animated buttons are marked dirty.
No animation wakeups occur on a static/Fn layer or while the backlight is dimmed
or off. Existing input, lid handling, adaptive brightness and idle timers remain
with tiny-dfr. Resource use must be measured on the target hardware rather than
inferred from the implementation alone.

The companion [Omarchy plugin](https://github.com/guirossibrum/omarchy-touchbar)
provides the logo geometry, default layout, pinned build/install helper, preview,
and rollback instructions. The daemon has no hard-coded Omarchy shape.

## Validation

```sh
cargo test --locked
cargo build --locked --release --bin tiny-dfr
target/release/tiny-dfr --capabilities
target/release/tiny-dfr --check-config
cargo run --locked --bin trace-preview -- /path/to/plugin/native/preview.toml /tmp/trace-frames
```

The preview uses the same Rust/Cairo renderer without opening DRM or requiring
root. Tests cover phase-boundary continuity, intermediate frames, changing origins,
bounded configuration, and completion of the real ttfx Wipe effect.

`--check-config` loads the installed templates, user overrides, fonts and icons
without opening DRM. Templates predating `DoublePressSwitchLayers` default that
optional setting to zero. The companion installer validates config before
activation and restores the packaged daemon if the new invocation reports a panic.
