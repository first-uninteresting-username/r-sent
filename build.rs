//! Copies `config.def.toml` to `config.toml` if needed and validates it, so that an invalid
//! configuration is a build error. `src/main.rs` embeds the validated `config.toml`.

use std::path::Path;
use std::process::exit;

#[path = "src/config.rs"]
#[allow(dead_code)]
mod config;

fn main() {
    println!("cargo:rerun-if-changed=config.def.toml");
    println!("cargo:rerun-if-changed=config.toml");
    println!("cargo:rerun-if-changed=src/config.rs");

    let dir = std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_else(|| ".".into());
    let dir = Path::new(&dir);
    let def = dir.join("config.def.toml");
    let user = dir.join("config.toml");

    if !user.exists()
        && let Err(e) = std::fs::copy(&def, &user)
    {
        eprintln!(
            "error: cannot copy {} to {}: {e}",
            def.display(),
            user.display()
        );
        exit(1);
    }

    let text = match std::fs::read_to_string(&user) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", user.display());
            exit(1);
        }
    };

    if let Err(e) = config::parse_str(&text) {
        eprintln!("error: invalid {}: {e}", user.display());
        exit(1);
    }
}
