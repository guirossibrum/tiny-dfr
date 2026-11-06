# Vector trace buttons using Omarchy's screensaver engine

This fork adds an optional `Trace` inline table to a button. Keep `Icon` as a
static fallback: unmodified tiny-dfr ignores the new field and shows that icon.
The button's action and Fn behavior do not change.

```toml
{ Icon = "omarchy-green", Action = "Search", Trace = { Paths = [[[0.1, 0.1], [0.9, 0.1], [0.9, 0.9]]], Color = [0.57, 0.80, 0.33], BaseColor = [0.4, 0.4, 0.4], LineWidth = 0.068, DurationSeconds = 3.2, PauseSeconds = 1.0 } }
```

Paths specify connected rails in normalized icon coordinates. Explicit points
that lie on another rail split it into a shared node; duplicated/reversed rails
are deduplicated. Disconnected graphs fall back to the static icon. Width is also
normalized. There may be at most 64 paths and 1024 points; colors and coordinates
must be finite and in 0..1. Duration is 0.5..120 seconds; pause is 0..60 seconds.
Invalid trace settings fall back to the static button with a diagnostic.

The **actual ttfx `fx::effects::wipe::Wipe` implementation** generates coverage
masks on a 128-cell, one-row virtual canvas when the icon loads. It uses ttfx's
Linear easing. The full engine is dropped after these masks are generated;
the per-frame code maps the masks to the vector path and paints with Cairo.
ttfx is pinned to a full Git revision in Cargo.toml and Cargo.lock. No code is
copied out of ttfx, and no terminal, subprocess, or animation thread runs.

Each pass alternates gray-to-green and green-to-gray. ttfx's RNG chooses 64
interior sources on eligible rails. Each source is at least `LineWidth` away from
both endpoint nodes, so it never starts on a node. Adjacent sources differ,
including across the 64-pass repeat boundary.

The renderer computes weighted shortest arrival distances from each source to
every graph node at load time. A source inside a rail launches two fronts. When
a front reaches a degree-3 junction it splits into two outgoing rails; a degree-4
junction splits into three. Fronts merge inside rails reached from both ends.
The last-arrival distance includes rail interiors, preventing leftover gaps on
loops. Per-frame work uses fixed-size, merged spans with no route search.

`DurationSeconds` is the time to traverse the graph's node-to-node diameter.
Travel speed is fixed across sources; pass duration varies with the last arrival,
then holds for `PauseSeconds` only when fully green (default one second).
There is no hold when fully gray: the next green-fill pass begins immediately. The real ttfx Linear Wipe curve is interpolated
between its samples to avoid the former circular easing acceleration. A pass
does not reset its layer, reload configuration, or reset held keys.

Animation is capped at roughly 15 fps. Only animated buttons are marked dirty.
Animation continues while the backlight is idle-dimmed (brightness 1).
No animation wakeups occur on a static/Fn layer or while the backlight is fully
off (brightness 0). Existing input, lid handling, adaptive brightness and idle timers remain
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

## Text alignment and optional holds

Text buttons use one font-metrics baseline, including labels with descenders,
and subtract the horizontal ink bearing when centering. The companion plugin
resolves the current user's Omarchy monospace font before writing `FontTemplate`.

An optional `HoldAction = "F15", HoldMilliseconds = 600` on a button defers its
normal action until a short tap releases. Crossing the threshold presses the
hold action exactly once; release lifts it without triggering the tap. The
threshold is 200..2000 ms. Sliding off, input cancellation, device removal,
layer changes, config reload, and a fully off backlight release/cancel holds.
Only virtual keycodes are emitted; the daemon never launches dictation commands.
The user-session plugin binds F15 press/release through Omarchy's backend-neutral
`omarchy dictation start` / `omarchy dictation stop` commands.
