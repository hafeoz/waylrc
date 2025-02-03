use std::{fs::File, io, sync::Mutex};

use clap::Parser;
use tracing_subscriber::EnvFilter;

/// Command line arguments
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Force a D-Bus sync every X seconds
    #[clap(long, short, default_value_t = 60.0)]
    pub refresh_every: f64,
    /// File to write the log to. If not specified, logs will be written to stderr.
    #[clap(long, short)]
    log_file: Option<String>,
    /// Skip writing metadata with specified key.
    /// Check `<https://www.freedesktop.org/wiki/Specifications/mpris-spec/metadata/>`
    /// for a list of common fields.
    #[clap(long, short, default_values_t = ["xesam:asText".to_string()])]
    pub skip_metadata: Vec<String>,
}

impl Args {
    /// Build the tracing subscriber using parameters from the command line arguments
    ///
    /// # Panics
    ///
    /// Panics if the log file cannot be opened.
    pub fn init_tracing_subscriber(&self) {
        let builder = tracing_subscriber::fmt()
            .pretty()
            .with_env_filter(EnvFilter::from_default_env());

        match self.log_file.as_ref() {
            None => builder.with_writer(io::stderr).init(),
            Some(f) => builder
                .with_writer(Mutex::new(File::create(f).unwrap()))
                .init(),
        }
    }
}
