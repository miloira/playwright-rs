//! Locating and launching the Playwright Node driver.
//!
//! The driver is the `playwright` / `playwright-core` npm package. We run it
//! with `node <cli.js> run-driver`, which starts a protocol server that reads
//! and writes framed JSON over stdio.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::{Child, Command};

use crate::connection::Connection;
use crate::error::{Error, Result};

/// Describes how to launch the Playwright driver.
#[derive(Debug, Clone)]
pub struct Driver {
    node: OsString,
    cli_js: PathBuf,
}

impl Driver {
    /// Builds a driver launcher, locating `node` and the driver `cli.js`.
    ///
    /// Resolution order for the driver script:
    /// 1. The `PLAYWRIGHT_CLI_JS` environment variable (full path to `cli.js`).
    /// 2. The `PLAYWRIGHT_DRIVER_DIR` environment variable (a directory
    ///    containing `node_modules/playwright[-core]/cli.js`).
    /// 3. A `driver/` folder next to the current working directory.
    /// 4. The crate's own `driver/` folder (useful during development).
    ///
    /// `node` is taken from the `PLAYWRIGHT_NODE` environment variable when set,
    /// otherwise the `node` found on `PATH`.
    pub fn locate() -> Result<Driver> {
        let node = std::env::var_os("PLAYWRIGHT_NODE").unwrap_or_else(|| OsString::from("node"));

        if let Some(cli) = std::env::var_os("PLAYWRIGHT_CLI_JS") {
            let cli = PathBuf::from(cli);
            if cli.is_file() {
                return Ok(Driver { node, cli_js: cli });
            }
            return Err(Error::Protocol(format!(
                "PLAYWRIGHT_CLI_JS does not point to a file: {}",
                cli.display()
            )));
        }

        let mut search_dirs: Vec<PathBuf> = Vec::new();
        if let Some(dir) = std::env::var_os("PLAYWRIGHT_DRIVER_DIR") {
            search_dirs.push(PathBuf::from(dir));
        }
        if let Ok(cwd) = std::env::current_dir() {
            search_dirs.push(cwd.join("driver"));
            search_dirs.push(cwd);
        }
        search_dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("driver"));

        for dir in &search_dirs {
            if let Some(cli) = find_cli_js(dir) {
                return Ok(Driver { node, cli_js: cli });
            }
        }

        Err(Error::Protocol(
            "could not locate the Playwright driver cli.js; set PLAYWRIGHT_CLI_JS or \
             install it with `npm install playwright-core` into a `driver/` folder"
                .into(),
        ))
    }

    /// Spawns the driver process and returns a [`Connection`] that owns the
    /// child process (killed when the last connection clone is dropped).
    pub fn spawn_connection(&self) -> Result<Connection> {
        let mut child = self.spawn_child()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Protocol("driver stdin was not captured".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Protocol("driver stdout was not captured".into()))?;
        Ok(Connection::with_child(stdout, stdin, Some(child)))
    }

    fn spawn_child(&self) -> Result<Child> {
        let child = Command::new(&self.node)
            .arg(&self.cli_js)
            .arg("run-driver")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                Error::Protocol(format!(
                    "failed to launch node driver ({}): {e}",
                    self.cli_js.display()
                ))
            })?;

        Ok(child)
    }
}

/// Looks for `node_modules/playwright[-core]/cli.js` under `dir`, or a `cli.js`
/// directly inside `dir`.
fn find_cli_js(dir: &Path) -> Option<PathBuf> {
    let candidates = [
        dir.join("node_modules/playwright/cli.js"),
        dir.join("node_modules/playwright-core/cli.js"),
        dir.join("cli.js"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}
