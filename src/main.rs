//! r-sent: a simple plaintext presentation tool for Wayland.

mod app;
mod config;
mod error;
mod font;
mod image;
mod layout;
mod parse;
mod render;
mod slide;
mod wayland;

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::app::{App, Source};
use crate::config::Config;
use crate::error::Error;

/// The validated `config.toml`, embedded at build time (see `build.rs`).
const EMBEDDED_CONFIG: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/config.toml"));

const USAGE: &str = "usage: r-sent [-v] [file] [config]";

#[derive(Debug, PartialEq, Eq)]
enum Cli {
    Version,
    Usage,
    Run {
        file: Option<OsString>,
        config: Option<OsString>,
    },
}

/// Parses the command line (without the program name).
fn parse_args(args: impl IntoIterator<Item = OsString>) -> Cli {
    let mut positional = Vec::new();
    let mut options_done = false;
    for arg in args {
        if !options_done && positional.is_empty() {
            if arg == "--" {
                options_done = true;
                continue;
            }
            let bytes = arg.as_encoded_bytes();
            if bytes.len() > 1 && bytes[0] == b'-' {
                if bytes[1..].iter().all(|&b| b == b'v') {
                    return Cli::Version;
                }
                return Cli::Usage;
            }
        }
        if positional.len() < 2 {
            positional.push(arg);
        }
    }
    let mut it = positional.into_iter();
    Cli::Run {
        file: it.next(),
        config: it.next(),
    }
}

/// `$XDG_CONFIG_HOME/r-sent/config.toml`, falling back to `~/.config/r-sent/config.toml`.
fn user_config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config")))?;
    Some(base.join("r-sent").join("config.toml"))
}

/// Loads the embedded configuration and applies overrides: the user configuration file, then
/// the file given on the command line.
fn load_config(cli: Option<&Path>) -> Result<Config, Error> {
    let embedded_err = |e: config::ConfigError| Error::Config {
        path: "<embedded config>".into(),
        msg: e.0,
    };
    let mut table = config::parse_table(EMBEDDED_CONFIG).map_err(embedded_err)?;

    let mut layers = Vec::new();
    if let Some(p) = user_config_path().filter(|p| p.exists()) {
        layers.push(p);
    }
    layers.extend(cli.map(Path::to_path_buf));

    for path in layers {
        let err = |msg: String| Error::Config {
            path: path.to_string_lossy().into_owned(),
            msg,
        };
        let text = std::fs::read_to_string(&path).map_err(|e| err(e.to_string()))?;
        let over = config::parse_table(&text).map_err(|e| err(e.0))?;
        config::merge(&mut table, over);
        // Validate after each layer so errors name the file that caused them.
        config::from_table(table.clone()).map_err(|e| err(e.0))?;
    }
    config::from_table(table).map_err(embedded_err)
}

fn run(file: Option<OsString>, config: Option<OsString>) -> Result<(), Error> {
    let config = load_config(config.as_deref().map(Path::new))?;
    let source = match file {
        None => Source::Stdin,
        Some(f) if f == OsStr::new("-") => Source::Stdin,
        Some(f) => Source::File(PathBuf::from(f)),
    };
    let slides = source.read_slides()?;
    wayland::run(App::new(config, source, slides))
}

fn main() -> ExitCode {
    match parse_args(std::env::args_os().skip(1)) {
        Cli::Version => {
            println!("r-sent-{}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Cli::Usage => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
        Cli::Run { file, config } => match run(file, config) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                error::report(&e);
                ExitCode::FAILURE
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Cli {
        parse_args(a.iter().map(OsString::from))
    }

    fn run_args(file: Option<&str>, config: Option<&str>) -> Cli {
        Cli::Run {
            file: file.map(OsString::from),
            config: config.map(OsString::from),
        }
    }

    #[test]
    fn cli_parsing() {
        assert_eq!(args(&[]), run_args(None, None));
        assert_eq!(args(&["-v"]), Cli::Version);
        assert_eq!(args(&["-vv"]), Cli::Version);
        assert_eq!(args(&["-x"]), Cli::Usage);
        assert_eq!(args(&["-vx"]), Cli::Usage);
        assert_eq!(args(&["--help"]), Cli::Usage);
        assert_eq!(args(&["-"]), run_args(Some("-"), None));
        assert_eq!(args(&["a.txt"]), run_args(Some("a.txt"), None));
        assert_eq!(
            args(&["a.txt", "c.toml"]),
            run_args(Some("a.txt"), Some("c.toml"))
        );
        assert_eq!(
            args(&["a.txt", "c.toml", "extra", "-x"]),
            run_args(Some("a.txt"), Some("c.toml"))
        );
        assert_eq!(args(&["--", "-file"]), run_args(Some("-file"), None));
        assert_eq!(args(&["--", "-v"]), run_args(Some("-v"), None));
        assert_eq!(args(&["--", "--"]), run_args(Some("--"), None));
        assert_eq!(args(&["a.txt", "-v"]), run_args(Some("a.txt"), Some("-v")));
    }

    #[test]
    fn non_utf8_paths() {
        use std::os::unix::ffi::OsStringExt;
        let name = OsString::from_vec(vec![b'f', 0xff]);
        assert_eq!(
            parse_args([name.clone()]),
            Cli::Run {
                file: Some(name),
                config: None
            }
        );
    }

    #[test]
    fn embedded_config_is_valid() {
        assert!(config::parse_str(EMBEDDED_CONFIG).is_ok());
    }
}
