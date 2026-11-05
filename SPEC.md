# r-sent — Specification

r-sent is a simple plaintext presentation tool for Wayland, written in Rust.

This document specifies the behaviour of r-sent completely enough to implement and test it.

---

## 1. Overview

- Slides are described in a plain text file. **Every paragraph is one slide.**
- A paragraph whose first line starts with `@` is an **image slide**.
- The presentation is shown in a single Wayland `xdg_toplevel` window, fullscreen by default. Text is scaled to the largest font size that
  fits and centred; images are scaled to fit and centred.
- Navigation is by keyboard and mouse. There is no editing, no transitions, no markup.

### Goals

- Minimal: a presentation is a text file, nothing more.
- Small, dependency-light, safe Rust (no `unsafe` outside what dependencies require).
- Configuration in TOML.
- Wayland-native: no X11 or XWayland dependency; correct on HiDPI and fractional-scale outputs.

### Non-goals

- X11, macOS or Windows support.
- Markup, styling per slide, transitions, presenter notes, PDF export.
- Text shaping (ligatures, complex scripts, bidi). Glyphs are laid out one codepoint at a time.

---

## 2. Command-line interface

```
r-sent [-v] [file] [config]
```

| Argument | Meaning                                                                          |
| -------- | -------------------------------------------------------------------------------- |
| `-v`     | Print `r-sent-<CARGO_PKG_VERSION>` to stdout and exit with status 0.             |
| `file`   | Presentation file. If omitted or equal to `-`, read the presentation from stdin. |
| `config` | Configuration file. If omitted, use the default configuration.                   |
| `--`     | Ends option parsing; the next argument is the file even if it starts with `-`.   |

Rules:

- Options may be combined (`-vv` is the same as `-v`).
- Any other option prints `usage: r-sent [-v] [file] [config]` to stderr and exits with status 1.
- Arguments after the first positional argument are ignored.
- Parsing is hand-written (no CLI crate needed); `std::env::args_os` is used so non-UTF-8
  paths work.

Exit status: `0` on normal quit (key binding or window close), `1` on any fatal error.

---

## 3. Presentation file format

The file is read as bytes and split into lines on `\n`. A trailing `\r` on a line is removed.
Each line is decoded as UTF-8; invalid sequences become U+FFFD. Lines have no length limit.

### 3.1 Line classification

| Line            | Meaning                                                                 |
| --------------- | ----------------------------------------------------------------------- |
| empty (`""`)    | **Separator**: ends the current slide. Consecutive separators collapse. |
| starts with `#` | **Comment**: dropped. It does _not_ end the current slide.              |
| anything else   | **Content line** of the current slide.                                  |

A line containing only whitespace is _not_ a separator; it is a content line.

### 3.2 Building a slide

For each content line, in order:

1. If it is the **first** line of the slide and starts with `@`, the slide is an image slide and
   the text after `@` (the whole rest of the line, verbatim, no trimming) is its image path.
2. If the line starts with `\`, exactly one leading `\` is removed. This happens _after_ step 1,
   so `\@foo.png` is the text line `@foo.png` and `\#x` is the text line `#x`. A line consisting
   only of `\` becomes an empty text line.
3. The line is appended to the slide's lines.

If the file yields no slides at all, r-sent exits with `r-sent: presentation contains no slides`.

### 3.3 Image slides

- Only the first line determines the image; further lines of an image slide are kept but never
  drawn.
- A first line of exactly `@` (empty path) is **not** an image slide: it is drawn as a text slide
  showing `@`.
- Relative image paths are resolved against the directory containing the presentation file
  (stdin: the current working directory).

### 3.4 Example

```
r-sent

@diagram.png

why plain text
- diffable
- no tooling
# comment, not shown
\# shown, starts with a hash
\@shown.png, not an image

\
```

Produces 4 slides: `["r-sent"]`, image `diagram.png`, a 5-line text slide, and one slide
containing a single empty line (an empty slide).

---

## 4. Configuration (`config.toml`)

| File              | Purpose                                        |
| ----------------- | ---------------------------------------------- |
| `config.def.toml` | Checked-in, documented defaults                |
| `config.toml`     | User copy, git-ignored, embedded at build time |

### 4.1 Load order

1. **Compile time** — `build.rs` copies `config.def.toml` to `config.toml` if the latter does not
   exist, parses and validates it, and embeds it in the binary. An invalid `config.toml` is a
   **build error** with the TOML error message. `build.rs` emits `cargo:rerun-if-changed` for
   both files.
2. **Run time flag (optional)** - if `config` flag is passed, it is parsed and overwrites the embedded config, same as the path option.
3. **Run time path (optional)** — if `$XDG_CONFIG_HOME/r-sent/config.toml` (fallback
   `~/.config/r-sent/config.toml`) exists, it is parsed and its keys override the embedded
   config. Tables are merged key-by-key; arrays (`fallbacks`, `keys`, `mouse`, `filters`) replace
   the embedded array entirely. A present but invalid file is fatal:
   `r-sent: <path>: <error>`.

Unknown keys are an error (`#[serde(deny_unknown_fields)]`) so typos are caught.

### 4.2 Schema and defaults (`config.def.toml`)

```toml
# r-sent configuration.
# Copy to config.toml and rebuild, or put overrides in ~/.config/r-sent/config.toml.

[fonts]
# Font families tried in order for every glyph.
fallbacks = ["dejavu sans", "roboto", "ubuntu"]
# Number of font sizes available for fitting.
scales = 42
# Size of scale i, in points: floor(base_size * scale_factor ^ i), i in [0, scales-1].
base_size = 10.0
scale_factor = 1.1288

[colors]
foreground = "#000000"
background = "#FFFFFF"

[window]
# Start fullscreen. If false, the compositor picks the size (default 1280x720 if it doesn't).
fullscreen = true

[layout]
# Distance between baselines, as a multiple of the font height.
line_spacing = 1.4
# Fraction of the window that content may use at most.
usable_width = 0.75
usable_height = 0.75

# Pointer bindings. button: "left", "middle", "right", "scroll-up", "scroll-down".
mouse = [
  { button = "left",        action = "advance", arg = 1 },
  { button = "right",       action = "advance", arg = -1 },
  { button = "scroll-up",   action = "advance", arg = -1 },
  { button = "scroll-down", action = "advance", arg = 1 },
]

# Key bindings. key: XKB keysym name (as accepted by xkb_keysym_from_name).
keys = [
  { key = "Escape",    action = "quit" },
  { key = "q",         action = "quit" },
  { key = "Right",     action = "advance", arg = 1 },
  { key = "Left",      action = "advance", arg = -1 },
  { key = "Return",    action = "advance", arg = 1 },
  { key = "space",     action = "advance", arg = 1 },
  { key = "BackSpace", action = "advance", arg = -1 },
  { key = "l",         action = "advance", arg = 1 },
  { key = "h",         action = "advance", arg = -1 },
  { key = "j",         action = "advance", arg = 1 },
  { key = "k",         action = "advance", arg = -1 },
  { key = "Down",      action = "advance", arg = 1 },
  { key = "Up",        action = "advance", arg = -1 },
  { key = "Next",      action = "advance", arg = 1 },
  { key = "Prior",     action = "advance", arg = -1 },
  { key = "n",         action = "advance", arg = 1 },
  { key = "p",         action = "advance", arg = -1 },
  { key = "r",         action = "reload" },
  { key = "f",         action = "toggle-fullscreen" },
]

# Image filters, tried in order; the first whose regex matches the image path is used.
# The command runs via `sh -c`, gets the image file on stdin and must write an image
# to stdout. Images matching no filter are decoded natively.
filters = [
  { regex = '\.ff\.bz2$', command = "bunzip2" },
]
```

### 4.3 Validation rules

| Key                                    | Constraint                                               |
| -------------------------------------- | -------------------------------------------------------- |
| `fonts.fallbacks`                      | non-empty                                                |
| `fonts.scales`                         | 1 ..= 256                                                |
| `fonts.base_size`                      | > 0                                                      |
| `fonts.scale_factor`                   | ≥ 1.0                                                    |
| `colors.*`                             | `#RRGGBB` or `#RGB`, case-insensitive                    |
| `layout.line_spacing`                  | > 0                                                      |
| `layout.usable_width`, `usable_height` | in (0, 1]                                                |
| `mouse[].button`                       | `left`, `middle`, `right`, `scroll-up`, `scroll-down`    |
| `keys[].key`                           | a known keysym name (resolved once at startup/build)     |
| `*.action`                             | `"quit"`, `"advance"`, `"reload"`, `"toggle-fullscreen"` |
| `*.arg`                                | required integer for `advance`; must be absent otherwise |
| `filters[].regex`                      | valid `regex` crate syntax; matched case-insensitively   |

In Rust, bindings deserialize into:

```rust
enum Action { Quit, Advance(i32), Reload, ToggleFullscreen }
enum Button { Left, Middle, Right, ScrollUp, ScrollDown }
struct KeyBinding { keysym: xkeysym::Keysym, action: Action }
struct MouseBinding { button: Button, action: Action }
struct Filter { regex: regex::Regex, command: String }
```

Several bindings may map the same key/button; all matching actions run in config order.

---

## 5. Architecture

Single binary crate, edition 2024.

```
build.rs          copy config.def.toml -> config.toml, validate, embed
config.def.toml   default configuration
src/
  main.rs         argument parsing, startup, top-level error -> exit(1)
  config.rs       serde types, defaults, validation, runtime override merge
  parse.rs        presentation parser (pure; no I/O beyond a BufRead)
  slide.rs        Slide / SlideKind types
  image.rs        image loading, filters, alpha pre-blending, scaling cache
  font.rs         font discovery, fallback chain, metrics, glyph rasterization
  layout.rs       font-size fitting and text placement (pure; testable with fake metrics)
  render.rs       software framebuffer, drawing text and images
  wayland.rs      connection, globals, xdg toplevel, seat input, scaling, shm buffers, event loop
  app.rs          application state (slides, index, source) and actions
  error.rs        error type and fatal-error formatting
```

### 5.1 Recommended crates

| Purpose                       | Crate                                                         |
| ----------------------------- | ------------------------------------------------------------- |
| Wayland client & protocols    | `smithay-client-toolkit` (with `calloop`, `xkbcommon`)        |
| Font discovery                | `fontdb` (system fonts)                                       |
| Glyph rasterization / metrics | `swash` (or `fontdue`)                                        |
| Keysym names & constants      | `xkbcommon` (`keysym_from_name`), `xkeysym`                   |
| Image decoding                | `image` (features: `png`, `jpeg`, `gif`, `ff`, `bmp`, `webp`) |
| Regex                         | `regex`                                                       |
| Config                        | `serde`, `toml`                                               |
| Errors                        | `thiserror` or plain enum                                     |

Runtime C dependencies are limited to `libwayland-client` and `libxkbcommon`.

### 5.2 Data model

```rust
struct Slide {
    lines: Vec<String>,
    kind: SlideKind,
}

enum SlideKind {
    Text,
    Image { path: PathBuf, image: Option<LoadedImage> },
}

struct LoadedImage {
    width: u32,
    height: u32,
    rgb: Vec<u8>,                    // width*height*3, alpha already blended with background
    scaled: Option<ScaledImage>,     // cache, see §8.4
}

enum Source { File(PathBuf), Stdin }

struct App {
    config: Config,
    source: Source,
    slides: Vec<Slide>,  // never empty
    idx: usize,
    running: bool,
    fullscreen: bool,
}
```

---

## 6. Window and Wayland behaviour

### 6.1 Startup sequence

1. Parse arguments, load config, read and parse the presentation (§3). Errors are fatal.
2. Connect to the compositor (`$WAYLAND_DISPLAY`). Failure:
   `r-sent: cannot connect to Wayland display: <err>`.
3. Bind globals. Required: `wl_compositor`, `wl_shm`, `xdg_wm_base`. Missing one:
   `r-sent: compositor lacks required protocol '<name>'`. Optional, used when present:
   `wl_seat` (input), `wp_fractional_scale_manager_v1` + `wp_viewporter` (fractional scaling),
   `zxdg_decoration_manager_v1` (server-side decorations).
4. Load fonts (§7.1) and all images (§8) — errors are fatal.
5. Create a `wl_surface` with an `xdg_toplevel`:
   - title `r-sent`, or `r-sent: <file name>` for file input
   - app id `r-sent`
   - `set_fullscreen(None)` if `window.fullscreen` is true
   - request server-side decorations if the decoration protocol is available; otherwise the
     window is undecorated (no client-side decorations are drawn)
6. Commit without a buffer and wait for the first `configure`; then draw (§6.4).
7. Enter the event loop (`calloop`, with the Wayland source and key-repeat timers).

### 6.2 Surface size and scale

- Logical size `w × h` comes from `xdg_toplevel.configure`. If the compositor sends `0 × 0`,
  use the last size, or 1280 × 720 initially.
- **Scale factor** `s`:
  - with `wp_fractional_scale_v1`: `preferred_scale / 120`; the buffer is attached with
    `wp_viewport.set_destination(w, h)` and buffer scale 1;
  - otherwise: the integer `wl_surface.preferred_buffer_scale` (or the maximum scale of the
    outputs the surface has entered, for older compositors), applied via
    `wl_surface.set_buffer_scale`.
- **Physical** size `pw = round(w * s)`, `ph = round(h * s)`. All layout and rendering in §7–§8
  use physical pixels: `w`, `h` there mean `pw`, `ph`, and `uw = floor(usable_width * pw)`,
  `uh = floor(usable_height * ph)`.
- A change of logical size or scale invalidates the current slide's scaled image and schedules
  a redraw.

### 6.3 Input

| Event                            | Behaviour                                                                                                                                                                                                                                                               |
| -------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `wl_keyboard.key` pressed        | Translate with the XKB keymap sent by the compositor. The keysym used for matching is the one at **shift level 0** of the active layout for that key (modifiers are ignored, so Shift+q still matches `q`). Run every key binding whose keysym equals it.               |
| key repeat                       | Repeats (using the compositor's `repeat_info`) are treated as further presses, so holding an advance key steps through slides.                                                                                                                                          |
| `wl_pointer.button` pressed      | `BTN_LEFT` → `left`, `BTN_MIDDLE` → `middle`, `BTN_RIGHT` → `right`. Run every matching mouse binding.                                                                                                                                                                  |
| `wl_pointer.axis` (vertical)     | One `scroll-up`/`scroll-down` per notch: per 120 units of `axis_value120`, or per `axis_discrete` step on older seats. Without discrete info (touchpads), accumulate the continuous value and emit one step per 50 logical units; reset the accumulator on `axis_stop`. |
| `xdg_toplevel.close`             | Stop.                                                                                                                                                                                                                                                                   |
| `xdg_toplevel.configure`         | Update size/fullscreen state (§6.2).                                                                                                                                                                                                                                    |
| keyboard/pointer capability lost | Release the device; no error.                                                                                                                                                                                                                                           |

Without a `wl_seat` the presentation still displays and can be closed via the compositor.

The loop ends when `running` becomes false; then all Wayland objects are destroyed, the
connection is flushed and closed, and the process exits 0. A lost connection or protocol error is
fatal.

### 6.4 Drawing to the surface

- All drawing goes to a client-side `pw × ph` framebuffer in `wl_shm` format `XRGB8888`
  (falling back to `ARGB8888` with alpha 0xFF), from a double-buffered shm pool (SCTK
  `SlotPool`); a buffer still held by the compositor is never written to.
- Drawing is on demand: after a slide change, reload, configure or scale change, request a
  `wl_surface.frame` callback and render when it fires (at most one frame per callback).
- Each commit attaches the buffer, damages the whole buffer (`damage_buffer`), and sets an
  opaque region covering the surface.

## 7. Text rendering

### 7.1 Fonts

- Font **scale i** (0 ≤ i < `scales`) has size `floor(base_size * scale_factor^i)` points.
  Pixel size = points × 96 / 72 × `s` (the surface scale factor, §6.2).
- The **fontset** is the list of faces resolved from `fonts.fallbacks` (regular weight, upright),
  skipping families not installed. If none resolves: `r-sent: none of the configured fonts are
installed`.
- **Glyph selection** per codepoint: first face in the fontset that has a glyph for it.
  If none does, search all system faces for one that covers it and, if found, append it to the
  fontset (cached for later lookups). If still none, use the first face's `.notdef` glyph.
- **Font height** `fh(i)` = ascent + descent of the _first_ face at scale i, in pixels
  (rounded up).
- **Text width** of a line = sum of horizontal advances of its glyphs, each from the face
  selected for it, at scale i. No kerning, no shaping.
- Color glyphs (emoji) should be drawn in color if the rasterizer supports it; otherwise as
  coverage masks in the foreground color.

### 7.2 Choosing the font size (fit algorithm)

For a slide with `n` lines (`n ≥ 1`):

```
lfac = line_spacing * (n - 1) + 1

# fit height
j = largest i in [0, scales-1] with fh(i) * lfac <= uh, or 0 if none

# fit width
for each line in order:
    while j > 0 and width(line, j) > uw:
        j -= 1

block_width  = max over lines of width(line, j)
block_height = floor(fh(j) * lfac)
```

Image slides skip this step.

### 7.3 Placing text

1. Fill the whole framebuffer with the background colour.
2. For line `k` (0-based):
   - `x = (w - block_width) / 2` (integer, may be negative → clip)
   - `y = (h - block_height) / 2 + floor(k * line_spacing * fh(j))`
   - The line is drawn in a box of width `block_width` and height `fh(j)` at `(x, y)`.
     All lines share the same `x`: the block is centred, lines are **left-aligned** within it.
   - For each run of glyphs from the same face, the baseline is
     `y + (fh(j) - face_height) / 2 + face_ascent`, so fallback faces of different height are
     vertically centred in the box.
3. **Truncation**: if a line is wider than `block_width` (only possible when even scale 0 is
   too wide), trailing characters are dropped until the line plus `...` fits, and `...` is
   appended.
4. Glyph coverage is alpha-blended over the background with the foreground colour.

---

## 8. Images

### 8.1 Loading

All image slides are loaded at startup, and again on reload, before anything is drawn:

1. Find the first filter whose `regex` matches the image path string (case-insensitive).
2. Open the file. Failure: `r-sent: cannot open image '<path>': <io error>`.
3. If a filter matched: spawn `sh -c <command>` with the file as stdin and a pipe as stdout;
   read stdout completely; wait for the child. Spawn failure or non-zero exit:
   `r-sent: filter '<command>' failed for '<path>': <reason>`. Decode the output, guessing the
   format from its magic bytes.
4. If no filter matched: decode the file natively with the `image` crate, guessing the format
   from content (falling back to the extension).
5. Decoding failure: `r-sent: cannot decode image '<path>': <reason>`.
   A zero width or height, or a size whose `width * height * 3` overflows `usize`:
   `r-sent: image '<path>' has invalid dimensions`.

### 8.2 Transparency

Windows are opaque, so images are converted to RGBA16 and pre-blended with the background
colour at load time:

```
c8 = c16 / 257          # per channel, integer
a8 = a16 / 257
out = (fg * a8 + bg * (255 - a8)) / 255     # per channel, integer arithmetic
```

The stored buffer is RGB8, row-major, no padding.

### 8.3 Scaling and placement

Target size (integer arithmetic, `bw × bh` = image size):

```
if uw * bh > uh * bw:  tw = bw * uh / bh;  th = uh
else:                  tw = uw;            th = bh * uw / bw
```

(Use `u64` intermediates.) Scaling is nearest-neighbour:
`src_x = x * bw / tw`, `src_y = y * bh / th`.

Drawing: fill the framebuffer with the background colour, then copy the scaled image to
`((w - tw) / 2, (h - th) / 2)`.

### 8.4 Scaled-image cache

The scaled image is cached on the slide. The cache is invalidated when the window is resized
and when navigation leaves the slide (so at most one scaled image is held at a time).

---

## 9. Actions

### `advance(n)`

`new = clamp(idx + n, 0, len - 1)`. If `new != idx`: drop the current slide's scaled-image
cache, set `idx = new`, redraw. Otherwise do nothing (no redraw).

### `toggle-fullscreen`

Call `xdg_toplevel.set_fullscreen` or `unset_fullscreen` depending on the current state reported
by the last `configure`. The actual size change arrives via `configure`.

### `quit`

Set `running = false`.

### `reload`

- Source is stdin: print `r-sent: reload is not available for stdin input` to stderr; do
  nothing.
- Otherwise re-read and re-parse the file and reload all images. On success, replace the slides,
  clamp `idx` to the new slide count, and redraw. On any error (unreadable file, no slides,
  image failure) print the error to stderr and **keep the previous presentation**.

---

## 10. Errors

All fatal errors print a single line `r-sent: <message>` to stderr and exit with status 1.
When the cause is an OS or library error, it is appended as `: <error>`. Errors propagate as
`Result` up to `main`; no `panic!`/`unwrap()` on input-, environment- or compositor-dependent paths.

| Situation               | Message                                       |
| ----------------------- | --------------------------------------------- |
| presentation unreadable | `cannot read '<file>': <err>`                 |
| no slides               | `presentation contains no slides`             |
| no compositor           | `cannot connect to Wayland display: <err>`    |
| missing global          | `compositor lacks required protocol '<name>'` |
| no font                 | `none of the configured fonts are installed`  |
| image errors            | see §8.1                                      |
| invalid runtime config  | `<path>: <err>`                               |

---

## 11. Build, packaging, documentation

- `cargo build --release` produces `target/release/r-sent`.
- `.gitignore` contains `config.toml` (the user's copy) and `target/`.
- `r-sent.1` man page (mdoc): synopsis, options, file format (§3), default key and mouse
  bindings, configuration (§4).
- `example/` directory: a demo presentation exercising every format feature (text, comments,
  escapes, empty slide, image slide, transparent image, wide Unicode/emoji line), with images
  created for this project.
- Development shell: devenv (`languages.rust.enable`, `rustfmt` and `clippy` hooks).
  `cargo clippy -- -D warnings` and `cargo fmt --check` must pass.
- A Nix package is built with `rustPlatform.buildRustPackage`; runtime fonts are found through
  the system font directories (`fontdb::Database::load_system_fonts`).

---

## 12. Testing

Unit tests (no X server required):

- **parse.rs** — separators, collapsed separators, comments inside and between paragraphs,
  `\` escapes (`\@`, `\#`, lone `\`), `@` image detection only on the first line, lone `@` is
  text, `\r\n` line endings, invalid UTF-8, no trailing newline, empty/comment-only input →
  error, whitespace-only line is content. `example/` must parse to the expected slide list.
- **config.rs** — `config.def.toml` parses and validates; every rule in §4.3 rejects bad input;
  override merging replaces arrays and merges tables; unknown keys rejected; keysym names
  resolve.
- **layout.rs** — fit algorithm against a fake font metric (e.g. `fh(i) = i+10`,
  `width = chars * (i+1)`): height-limited, width-limited, single huge line falls to scale 0
  and is truncated with `...`, multi-line block positions and left alignment.
- **image.rs** — alpha blending formula on known pixels, target-size computation for wide/tall
  images, nearest-neighbour scaling on a small grid, filter selection order and
  case-insensitivity, zero-size image rejected, filter command failure reported.
- **app.rs** — `advance` clamping and no-op when unchanged, reload from stdin rejected, reload
  failure keeps the old slides, reload clamps `idx`.

- **wayland.rs (pure parts)** — scale and physical-size computation, scroll accumulation
  (value120, discrete, continuous), evdev button mapping.

Integration (optional, gated behind an env var): run under a headless compositor
(`sway` with `WLR_BACKENDS=headless` or `weston --backend=headless`), inject input, and compare
screenshots.
