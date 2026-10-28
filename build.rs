//! Build script for the `dashboard` feature (enabled by default).
//!
//! The React dashboard in `frontend/` is embedded into the binary via
//! `rust-embed`, which requires `frontend/dist` to exist at compile time.
//! A fresh clone (or a crates.io tarball) does not ship `frontend/dist`,
//! so this script:
//!
//! 1. Skips everything when the `dashboard` feature is disabled.
//! 2. Skips the rebuild when `frontend/dist` is already newer than every
//!    frontend source file (fast no-op for repeat `cargo build`s).
//! 3. Otherwise, builds the frontend automatically with the user's installed
//!    Node.js/npm (`npm ci` with a fallback to `npm install`, then `npm run build`).
//! 4. Fails the build with a clear, actionable message if Node.js/npm is not
//!    available, or if the build produces no assets.
//!
//! Escape hatch: set `COMFY_SERVE_SKIP_FRONTEND=1` to skip the npm build
//! entirely (e.g. offline builds or cross-compilation). An empty
//! `frontend/dist` is then created so the crate still compiles; the binary
//! will simply serve an empty dashboard.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    embed_windows_resource();

    println!("cargo:rerun-if-env-changed=COMFY_SERVE_SKIP_FRONTEND");
    println!("cargo:rerun-if-changed=build.rs");

    // Only needed when the dashboard (rust-embed) feature is enabled.
    if env::var_os("CARGO_FEATURE_DASHBOARD").is_none() {
        return;
    }

    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set"));
    let frontend_dir = manifest_dir.join("frontend");
    let dist_dir = frontend_dir.join("dist");
    let dist_index = dist_dir.join("index.html");

    // Tell cargo when to re-run this script. Frontend sources live under these
    // paths; `dist` and `node_modules` are deliberately not watched (we write
    // to `dist` ourselves, so watching it would cause re-run loops).
    for watched in [
        "frontend/src",
        "frontend/public",
        "frontend/index.html",
        "frontend/package.json",
        "frontend/package-lock.json",
        "frontend/vite.config.ts",
        "frontend/tsconfig.json",
        "frontend/tsconfig.app.json",
        "frontend/tsconfig.node.json",
    ] {
        println!("cargo:rerun-if-changed={}", watched);
    }

    // Explicit opt-out: keep (or create) whatever dist is on disk and move on.
    if env::var_os("COMFY_SERVE_SKIP_FRONTEND").is_some() {
        if let Err(err) = fs::create_dir_all(&dist_dir) {
            fail(&format!(
                "could not create the directory {} ({})\n\
                 COMFY_SERVE_SKIP_FRONTEND is set but rust-embed still needs \
                 the folder to exist.",
                dist_dir.display(),
                err
            ));
        }
        if !dist_index.exists() {
            eprintln!(
                "cargo:warning=COMFY_SERVE_SKIP_FRONTEND is set and frontend/dist is empty; \
                 the binary will compile but the embedded dashboard will have no assets."
            );
        }
        return;
    }

    if !frontend_dir.join("package.json").exists() {
        fail(&format!(
            "the `dashboard` feature (enabled by default) requires the frontend \
             sources in {}, but no package.json was found there.\n\
             Build without the dashboard instead: cargo build --no-default-features",
            frontend_dir.display()
        ));
    }

    // Nothing to do if dist is already up to date.
    if dist_index.exists() && dist_is_fresh(&frontend_dir, &dist_index) {
        return;
    }

    if npm_version(&frontend_dir).is_none() {
        fail(
            "the `dashboard` feature (enabled by default) needs Node.js/npm to build \
             the React frontend in frontend/, but npm was not found on PATH.\n\
             \n\
             Options:\n\
             \x20 1. Install Node.js (v20.19+ or v22.12+, https://nodejs.org/) and re-run the build.\n\
             \x20 2. Build without the dashboard: cargo build --no-default-features\n\
             \x20 3. Build the frontend manually (cd frontend && npm install && npm run build), \
             then set COMFY_SERVE_SKIP_FRONTEND=1 for offline/cached builds.",
        );
    }

    eprintln!("comfy-serve: building dashboard frontend in {}", frontend_dir.display());

    // Install dependencies only when node_modules is missing. Prefer a
    // deterministic `npm ci`; fall back to `npm install` when the lockfile is
    // out of sync with package.json.
    let mut installed = false;
    if !frontend_dir.join("node_modules").exists() {
        let install_ok = if frontend_dir.join("package-lock.json").exists() {
            run_npm(&frontend_dir, &["ci"]).unwrap_or(false)
        } else {
            run_npm(&frontend_dir, &["install"]).unwrap_or(false)
        };
        if !install_ok && !run_npm(&frontend_dir, &["install"]).unwrap_or(false) {
            fail("npm failed to install the frontend dependencies (see the npm output above).");
        }
        installed = true;
    }

    if !run_npm(&frontend_dir, &["run", "build"]).unwrap_or(false) {
        // A stale node_modules (deps added since it was last installed) is the
        // most common cause; try one reinstall-and-retry before giving up.
        if installed || !run_npm(&frontend_dir, &["install"]).unwrap_or(false) {
            fail("npm run build failed for the dashboard frontend (see the npm output above).");
        }
        if !run_npm(&frontend_dir, &["run", "build"]).unwrap_or(false) {
            fail("npm run build failed for the dashboard frontend (see the npm output above).");
        }
    }

    if !dist_index.exists() {
        fail(
            "npm run build reported success but frontend/dist/index.html is missing.\n\
             The frontend build did not produce the assets rust-embed expects.",
        );
    }
}

/// Windows-only: embeds the application icon and version info into the
/// executable (Explorer icon, file properties, task manager). Linux and macOS
/// binaries do not embed icons, so this is a no-op on other targets.
#[cfg(windows)]
fn embed_windows_resource() {
    println!("cargo:rerun-if-changed=resources/comfy-serve.ico");

    // WindowsResource::new() already populates FILEVERSION/PRODUCTVERSION and
    // the FileVersion/ProductVersion strings from CARGO_PKG_VERSION*.
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());

    let mut res = winres::WindowsResource::new();
    res.set_icon("resources/comfy-serve.ico");
    res.set("ProductName", "comfy-serve");
    res.set("FileDescription", env!("CARGO_PKG_DESCRIPTION"));
    res.set("FileVersion", &version);
    res.set("ProductVersion", &version);
    res.set(
        "LegalCopyright",
        &format!("MIT License - {}", env!("CARGO_PKG_AUTHORS")),
    );
    res.set("OriginalFilename", "comfy-serve.exe");

    if let Err(err) = res.compile() {
        eprintln!(
            "error: failed to compile the Windows executable resources ({err}).\n\
             This usually means the Windows resource compiler (rc.exe / windres) \n\
             could not be found for the {} target.",
            env::var("TARGET").unwrap_or_else(|_| "?".to_string())
        );
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn embed_windows_resource() {}

/// Runs `npm <args>` in `dir`, returning the exit status (None if npm could
/// not be spawned at all).
fn run_npm(dir: &Path, args: &[&str]) -> Option<bool> {
    let mut cmd = npm_base();
    cmd.args(args).current_dir(dir);
    match cmd.status() {
        Ok(status) => Some(status.success()),
        Err(_) => None,
    }
}

/// Returns `npm --version` output if npm is available and working.
fn npm_version(dir: &Path) -> Option<String> {
    let mut cmd = npm_base();
    cmd.arg("--version").current_dir(dir);
    let output = cmd.output().ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

fn npm_base() -> Command {
    // On Windows, npm is npm.cmd, which CreateProcess cannot spawn directly.
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "npm"]);
        cmd
    }
    #[cfg(not(windows))]
    {
        Command::new("npm")
    }
}

/// True when `dist_index` is at least as new as every tracked frontend source
/// file. `node_modules`, `dist`, and other directories without sources are
/// skipped.
fn dist_is_fresh(frontend_dir: &Path, dist_index: &Path) -> bool {
    let Ok(dist_mtime) = dist_index.metadata().and_then(|m| m.modified()) else {
        return false;
    };
    let Some(newest) = newest_source_mtime(frontend_dir) else {
        return true;
    };
    newest <= dist_mtime
}

fn newest_source_mtime(dir: &Path) -> Option<std::time::SystemTime> {
    let mut newest: Option<std::time::SystemTime> = None;
    let mut entries: Vec<PathBuf> = Vec::new();
    if let Ok(read_dir) = fs::read_dir(dir) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == "node_modules" || name == "dist" || name.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                entries.push(path);
            } else if let Ok(meta) = path.metadata() {
                if let Ok(mtime) = meta.modified() {
                    if newest.is_none_or(|n| mtime > n) {
                        newest = Some(mtime);
                    }
                }
            }
        }
    }
    for sub in entries {
        if let Some(mtime) = newest_source_mtime(&sub) {
            if newest.is_none_or(|n| mtime > n) {
                newest = Some(mtime);
            }
        }
    }
    newest
}

fn fail(reason: &str) -> ! {
    eprintln!("error: {}\nexited.", reason);
    std::process::exit(1);
}
