use std::{fs::File, io, sync::Mutex};

use clap::Parser;

/// Command line arguments
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Maximum number of millisecond to wait between lyric refreshes
    #[clap(long, short, default_value_t = 1000)]
    pub max_wait: u64,
    /// File to write the log to. If not specified, logs will be written to stderr.
    #[clap(long, short)]
    log_file: Option<String>,
}

impl Args {
    /// Build the tracing subscriber using parameters from the command line arguments
    ///
    /// # Panics
    ///
    /// Panics if the log file cannot be opened.
    pub fn init_tracing_subscriber(&self) {
        let builder = tracing_subscriber::fmt().pretty();

        match &self.log_file {
            None => builder.with_writer(io::stderr).init(),
            Some(f) => builder
                .with_writer(Mutex::new(File::create(f).unwrap()))
                .init(),
        }
    }
}
