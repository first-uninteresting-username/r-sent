//! Configuration: serde types, validation and runtime override merging.
//!
//! This module is self-contained (it does not refer to other modules of the crate) because
//! `build.rs` includes it to validate the embedded `config.toml` at compile time.

use std::fmt;

use regex::{Regex, RegexBuilder};
use serde::Deserialize;
use toml::{Table, Value};
use xkeysym::Keysym;

/// A configuration error; the message is shown to the user as is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

fn err<T>(msg: impl Into<String>) -> Result<T, ConfigError> {
    Err(ConfigError(msg.into()))
}

/// An RGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    /// The colour as a `0x00RRGGBB` pixel value.
    pub fn xrgb(self) -> u32 {
        (u32::from(self.r) << 16) | (u32::from(self.g) << 8) | u32::from(self.b)
    }

    /// Parses `#RRGGBB` or `#RGB` (case-insensitive).
    pub fn parse(s: &str) -> Option<Rgb> {
        let hex = s.strip_prefix('#')?;
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let digit = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).ok();
        match hex.len() {
            6 => Some(Rgb {
                r: u8::from_str_radix(&hex[0..2], 16).ok()?,
                g: u8::from_str_radix(&hex[2..4], 16).ok()?,
                b: u8::from_str_radix(&hex[4..6], 16).ok()?,
            }),
            3 => Some(Rgb {
                r: digit(0)? * 17,
                g: digit(1)? * 17,
                b: digit(2)? * 17,
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    Advance(i32),
    Reload,
    ToggleFullscreen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
    ScrollUp,
    ScrollDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyBinding {
    pub keysym: Keysym,
    pub action: Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseBinding {
    pub button: Button,
    pub action: Action,
}

#[derive(Debug, Clone)]
pub struct Filter {
    pub regex: Regex,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fonts {
    pub fallbacks: Vec<String>,
    pub scales: usize,
    pub base_size: f64,
    pub scale_factor: f64,
}

impl Fonts {
    /// Size of scale `i` in points: `floor(base_size * scale_factor ^ i)`.
    pub fn points(&self, i: usize) -> f64 {
        (self.base_size * self.scale_factor.powi(i as i32)).floor()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colors {
    pub foreground: Rgb,
    pub background: Rgb,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub line_spacing: f64,
    pub usable_width: f64,
    pub usable_height: f64,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub fonts: Fonts,
    pub colors: Colors,
    pub fullscreen: bool,
    pub layout: Layout,
    pub mouse: Vec<MouseBinding>,
    pub keys: Vec<KeyBinding>,
    pub filters: Vec<Filter>,
}

// ---------------------------------------------------------------------------------------------
// Raw (serde) representation.

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    fonts: RawFonts,
    colors: RawColors,
    window: RawWindow,
    layout: RawLayout,
    mouse: Vec<RawMouse>,
    keys: Vec<RawKey>,
    filters: Vec<RawFilter>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFonts {
    fallbacks: Vec<String>,
    scales: i64,
    base_size: f64,
    scale_factor: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawColors {
    foreground: String,
    background: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWindow {
    fullscreen: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLayout {
    line_spacing: f64,
    usable_width: f64,
    usable_height: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMouse {
    button: String,
    action: String,
    arg: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawKey {
    key: String,
    action: String,
    arg: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFilter {
    regex: String,
    command: String,
}

// ---------------------------------------------------------------------------------------------
// Parsing and validation.

/// Parses a TOML document into a table.
pub fn parse_table(s: &str) -> Result<Table, ConfigError> {
    s.parse::<Table>()
        .map_err(|e| ConfigError(e.to_string().trim_end().to_owned()))
}

/// Parses and validates a complete configuration document.
#[cfg_attr(not(test), allow(dead_code))] // used by build.rs and tests
pub fn parse_str(s: &str) -> Result<Config, ConfigError> {
    from_table(parse_table(s)?)
}

/// Deserializes and validates a complete configuration table.
pub fn from_table(table: Table) -> Result<Config, ConfigError> {
    let raw: RawConfig = Value::Table(table)
        .try_into()
        .map_err(|e: toml::de::Error| ConfigError(e.to_string().trim_end().to_owned()))?;
    validate(raw)
}

/// Merges `over` into `base`: tables are merged key by key, everything else (including
/// arrays) replaces the base value.
pub fn merge(base: &mut Table, over: Table) {
    for (key, value) in over {
        match (base.get_mut(&key), value) {
            (Some(Value::Table(b)), Value::Table(o)) => merge(b, o),
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

fn parse_action(ctx: &str, action: &str, arg: Option<i64>) -> Result<Action, ConfigError> {
    let no_arg = |a: Action| match arg {
        None => Ok(a),
        Some(_) => err(format!("{ctx}: action '{action}' does not take an 'arg'")),
    };
    match action {
        "quit" => no_arg(Action::Quit),
        "reload" => no_arg(Action::Reload),
        "toggle-fullscreen" => no_arg(Action::ToggleFullscreen),
        "advance" => match arg {
            None => err(format!("{ctx}: action 'advance' requires an integer 'arg'")),
            Some(n) => i32::try_from(n)
                .map(Action::Advance)
                .or_else(|_| err(format!("{ctx}: 'arg' {n} is out of range"))),
        },
        other => err(format!(
            "{ctx}: unknown action '{other}' \
             (expected quit, advance, reload or toggle-fullscreen)"
        )),
    }
}

fn parse_button(ctx: &str, s: &str) -> Result<Button, ConfigError> {
    Ok(match s {
        "left" => Button::Left,
        "middle" => Button::Middle,
        "right" => Button::Right,
        "scroll-up" => Button::ScrollUp,
        "scroll-down" => Button::ScrollDown,
        other => {
            return err(format!(
                "{ctx}: unknown button '{other}' \
                 (expected left, middle, right, scroll-up or scroll-down)"
            ));
        }
    })
}

/// Resolves an XKB keysym name (as accepted by `xkb_keysym_from_name`).
pub fn keysym_from_name(name: &str) -> Option<Keysym> {
    if name.is_empty() || name.contains('\0') {
        return None;
    }
    let sym = xkbcommon::xkb::keysym_from_name(name, xkbcommon::xkb::KEYSYM_NO_FLAGS);
    (sym.raw() != 0).then_some(Keysym::new(sym.raw()))
}

fn parse_color(ctx: &str, s: &str) -> Result<Rgb, ConfigError> {
    Rgb::parse(s).ok_or_else(|| {
        ConfigError(format!(
            "{ctx}: invalid colour '{s}' (expected #RRGGBB or #RGB)"
        ))
    })
}

fn validate(raw: RawConfig) -> Result<Config, ConfigError> {
    let f = raw.fonts;
    if f.fallbacks.is_empty() {
        return err("fonts.fallbacks: must not be empty");
    }
    if !(1..=256).contains(&f.scales) {
        return err(format!("fonts.scales: {} is not in 1..=256", f.scales));
    }
    if !(f.base_size > 0.0 && f.base_size.is_finite()) {
        return err(format!("fonts.base_size: {} must be > 0", f.base_size));
    }
    if !(f.scale_factor >= 1.0 && f.scale_factor.is_finite()) {
        return err(format!(
            "fonts.scale_factor: {} must be >= 1.0",
            f.scale_factor
        ));
    }

    let colors = Colors {
        foreground: parse_color("colors.foreground", &raw.colors.foreground)?,
        background: parse_color("colors.background", &raw.colors.background)?,
    };

    let l = raw.layout;
    if !(l.line_spacing > 0.0 && l.line_spacing.is_finite()) {
        return err(format!(
            "layout.line_spacing: {} must be > 0",
            l.line_spacing
        ));
    }
    for (name, v) in [
        ("usable_width", l.usable_width),
        ("usable_height", l.usable_height),
    ] {
        if !(v > 0.0 && v <= 1.0) {
            return err(format!("layout.{name}: {v} is not in (0, 1]"));
        }
    }

    let mouse = raw
        .mouse
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let ctx = format!("mouse[{i}]");
            Ok(MouseBinding {
                button: parse_button(&ctx, &m.button)?,
                action: parse_action(&ctx, &m.action, m.arg)?,
            })
        })
        .collect::<Result<Vec<_>, ConfigError>>()?;

    let keys = raw
        .keys
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let ctx = format!("keys[{i}]");
            let keysym = keysym_from_name(&k.key)
                .ok_or_else(|| ConfigError(format!("{ctx}: unknown keysym name '{}'", k.key)))?;
            Ok(KeyBinding {
                keysym,
                action: parse_action(&ctx, &k.action, k.arg)?,
            })
        })
        .collect::<Result<Vec<_>, ConfigError>>()?;

    let filters = raw
        .filters
        .into_iter()
        .enumerate()
        .map(|(i, flt)| {
            let regex = RegexBuilder::new(&flt.regex)
                .case_insensitive(true)
                .build()
                .map_err(|e| ConfigError(format!("filters[{i}].regex: {e}")))?;
            Ok(Filter {
                regex,
                command: flt.command,
            })
        })
        .collect::<Result<Vec<_>, ConfigError>>()?;

    Ok(Config {
        fonts: Fonts {
            fallbacks: f.fallbacks,
            scales: f.scales as usize,
            base_size: f.base_size,
            scale_factor: f.scale_factor,
        },
        colors,
        fullscreen: raw.window.fullscreen,
        layout: Layout {
            line_spacing: l.line_spacing,
            usable_width: l.usable_width,
            usable_height: l.usable_height,
        },
        mouse,
        keys,
        filters,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEF: &str = include_str!("../config.def.toml");

    fn with(over: &str) -> Result<Config, ConfigError> {
        let mut base = parse_table(DEF).unwrap();
        merge(&mut base, parse_table(over).unwrap());
        from_table(base)
    }

    fn rejects(over: &str, needle: &str) {
        match with(over) {
            Ok(_) => panic!("accepted invalid config:\n{over}"),
            Err(e) => assert!(e.0.contains(needle), "error {:?} lacks {needle:?}", e.0),
        }
    }

    #[test]
    fn default_config_parses() {
        let c = parse_str(DEF).unwrap();
        assert_eq!(c.fonts.fallbacks, ["dejavu sans", "roboto", "ubuntu"]);
        assert_eq!(c.fonts.scales, 42);
        assert_eq!(c.fonts.points(0), 10.0);
        assert_eq!(c.fonts.points(1), 11.0);
        assert_eq!(c.colors.foreground, Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(
            c.colors.background,
            Rgb {
                r: 255,
                g: 255,
                b: 255
            }
        );
        assert!(c.fullscreen);
        assert_eq!(c.layout.line_spacing, 1.4);
        assert_eq!(c.mouse.len(), 4);
        assert_eq!(
            c.mouse[0],
            MouseBinding {
                button: Button::Left,
                action: Action::Advance(1)
            }
        );
        assert_eq!(c.keys.len(), 19);
        assert_eq!(c.keys[0].keysym, Keysym::Escape);
        assert_eq!(c.keys[0].action, Action::Quit);
        assert_eq!(c.keys[17].action, Action::Reload);
        assert_eq!(c.keys[18].action, Action::ToggleFullscreen);
        assert_eq!(c.filters.len(), 1);
        assert!(c.filters[0].regex.is_match("x.FF.BZ2"));
    }

    #[test]
    fn keysym_names_resolve() {
        assert_eq!(keysym_from_name("Escape"), Some(Keysym::Escape));
        assert_eq!(keysym_from_name("q"), Some(Keysym::q));
        assert_eq!(keysym_from_name("Q"), Some(Keysym::Q));
        assert_eq!(keysym_from_name("Next"), Some(Keysym::Next));
        assert_eq!(keysym_from_name("Prior"), Some(Keysym::Prior));
        assert_eq!(keysym_from_name("BackSpace"), Some(Keysym::BackSpace));
        assert_eq!(keysym_from_name("space"), Some(Keysym::space));
        assert_eq!(keysym_from_name("NotAKey"), None);
        assert_eq!(keysym_from_name(""), None);
        assert_eq!(keysym_from_name("a\0b"), None);
    }

    #[test]
    fn colors() {
        assert_eq!(
            Rgb::parse("#fFf"),
            Some(Rgb {
                r: 255,
                g: 255,
                b: 255
            })
        );
        assert_eq!(
            Rgb::parse("#123"),
            Some(Rgb {
                r: 0x11,
                g: 0x22,
                b: 0x33
            })
        );
        assert_eq!(
            Rgb::parse("#a0B1c2"),
            Some(Rgb {
                r: 0xa0,
                g: 0xb1,
                b: 0xc2
            })
        );
        for bad in [
            "", "#", "fff", "#ffff", "#gggggg", "#12345", "#1234567", "#+1+2+3",
        ] {
            assert_eq!(Rgb::parse(bad), None, "{bad}");
        }
        assert_eq!(Rgb { r: 1, g: 2, b: 3 }.xrgb(), 0x010203);
    }

    #[test]
    fn validation_rules() {
        rejects("fonts.fallbacks = []", "fonts.fallbacks");
        rejects("fonts.scales = 0", "fonts.scales");
        rejects("fonts.scales = 257", "fonts.scales");
        assert!(with("fonts.scales = 256").is_ok());
        assert!(with("fonts.scales = 1").is_ok());
        rejects("fonts.base_size = 0.0", "fonts.base_size");
        rejects("fonts.base_size = -1.0", "fonts.base_size");
        rejects("fonts.scale_factor = 0.99", "fonts.scale_factor");
        assert!(with("fonts.scale_factor = 1.0").is_ok());
        rejects("colors.foreground = 'red'", "colors.foreground");
        rejects("colors.background = '#12'", "colors.background");
        assert!(with("colors.background = '#AbC'").is_ok());
        rejects("layout.line_spacing = 0.0", "layout.line_spacing");
        rejects("layout.usable_width = 0.0", "layout.usable_width");
        rejects("layout.usable_width = 1.01", "layout.usable_width");
        rejects("layout.usable_height = -0.5", "layout.usable_height");
        assert!(with("layout.usable_height = 1.0").is_ok());
        rejects(
            "mouse = [{ button = 'side', action = 'quit' }]",
            "unknown button",
        );
        rejects(
            "keys = [{ key = 'NoSuchKey', action = 'quit' }]",
            "unknown keysym",
        );
        rejects("keys = [{ key = 'q', action = 'jump' }]", "unknown action");
        rejects(
            "keys = [{ key = 'q', action = 'advance' }]",
            "requires an integer",
        );
        rejects("keys = [{ key = 'q', action = 'advance', arg = 'x' }]", "");
        rejects(
            "keys = [{ key = 'q', action = 'advance', arg = 9999999999 }]",
            "out of range",
        );
        rejects(
            "keys = [{ key = 'q', action = 'quit', arg = 1 }]",
            "does not take",
        );
        rejects(
            "mouse = [{ button = 'left', action = 'reload', arg = 1 }]",
            "does not take",
        );
        rejects(
            "filters = [{ regex = '(', command = 'cat' }]",
            "filters[0].regex",
        );
    }

    #[test]
    fn unknown_keys_rejected() {
        rejects("foo = 1", "foo");
        rejects("fonts.size = 1", "size");
        rejects(
            "keys = [{ key = 'q', action = 'quit', mods = 'ctrl' }]",
            "mods",
        );
    }

    #[test]
    fn merge_replaces_arrays_and_merges_tables() {
        let c = with(
            "
            [fonts]
            fallbacks = ['monospace']
            [colors]
            background = '#000'
            [[keys]]
            key = 'x'
            action = 'quit'
            ",
        )
        .unwrap();
        assert_eq!(c.fonts.fallbacks, ["monospace"]);
        // Untouched keys of a merged table keep their defaults.
        assert_eq!(c.fonts.scales, 42);
        assert_eq!(c.colors.foreground, Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(c.colors.background, Rgb { r: 0, g: 0, b: 0 });
        // Arrays are replaced, not appended.
        assert_eq!(c.keys.len(), 1);
        assert_eq!(c.keys[0].keysym, Keysym::x);
        assert_eq!(c.mouse.len(), 4);
        assert_eq!(c.filters.len(), 1);

        let c = with("filters = []").unwrap();
        assert!(c.filters.is_empty());
    }

    #[test]
    fn filters_case_insensitive() {
        let c = with("filters = [{ regex = '\\.png$', command = 'cat' }]").unwrap();
        assert!(c.filters[0].regex.is_match("A.PNG"));
        assert!(!c.filters[0].regex.is_match("a.png.jpg"));
    }

    #[test]
    fn invalid_toml_reported() {
        assert!(parse_table("fonts = [").is_err());
    }
}
