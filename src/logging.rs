use std::fs::{self, OpenOptions};
use std::path::Path;
use std::sync::Mutex;

use color_eyre::eyre::{Result, WrapErr};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::writer::MakeWriterExt;

const DEFAULT_FILTER: &str = "ss=info";

/// Logs to stderr and, for the app, appends to `<data>/logs/ss.log`.
/// `RUST_LOG` overrides the default `ss=info` filter.
pub fn init(log_file: Option<&Path>) -> Result<()> {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false);
    match log_file {
        Some(path) => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .wrap_err_with(|| format!("could not create {}", parent.display()))?;
            }
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .wrap_err_with(|| format!("could not open {}", path.display()))?;
            builder
                .with_writer(std::io::stderr.and(Mutex::new(file)))
                .init();
        }
        None => builder.with_writer(std::io::stderr).init(),
    }
    Ok(())
}
