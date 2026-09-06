pub mod agent;
pub mod board;
pub mod doc;
pub mod export;
pub mod framework;
pub mod import;
pub mod ingest;
pub mod mcp;
pub mod project;
pub mod skill;
pub mod sync;
pub mod workflow;

use std::io::Read;
use std::path::{Path, PathBuf};

/// Resolves a path argument to an absolute path, defaulting to the working
/// directory. The daemon does the work and its working directory is not the
/// caller's, so a relative path has to be resolved here or it means something
/// else by the time it arrives.
pub fn abs_path(path: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let path = match path {
        Some(p) => p,
        None => std::env::current_dir()?,
    };
    // A path that cannot be canonicalised (it does not exist yet) is passed on
    // as given, so the daemon reports the real problem rather than this layer.
    Ok(std::fs::canonicalize(&path).unwrap_or(path))
}

/// Reads a `--instructions-file`/`--body-file` argument: a file path, or `-`
/// for standard input.
pub fn read_source(path: &Path) -> anyhow::Result<String> {
    if path == Path::new("-") {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        return Ok(buf);
    }
    std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))
}

/// Prints a value as pretty JSON, the shape every `show` command answers in.
pub fn print_json<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Prints `rows` under `headers`, every column padded to its widest cell. Each
/// row must have one cell per header.
pub fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    let line = |cells: &[String]| {
        let mut out = String::new();
        for (i, cell) in cells.iter().enumerate() {
            out.push_str(cell);
            if i + 1 < cells.len() {
                out.push_str(&" ".repeat(widths[i] - cell.chars().count() + 2));
            }
        }
        println!("{}", out.trim_end());
    };
    line(&headers.iter().map(|h| h.to_string()).collect::<Vec<_>>());
    for row in rows {
        line(row);
    }
}
