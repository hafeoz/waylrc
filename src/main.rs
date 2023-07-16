#![feature(result_option_inspect)]
#![warn(
    clippy::pedantic,
    clippy::negative_feature_names,
    clippy::redundant_feature_names,
    clippy::wildcard_dependencies,
    clippy::allow_attributes_without_reason,
    clippy::clone_on_ref_ptr,
    clippy::default_union_representation,
    clippy::empty_structs_with_brackets,
    clippy::fn_to_numeric_cast_any,
    clippy::format_push_string,
    clippy::if_then_some_else_none,
    clippy::lossy_float_literal,
    clippy::missing_assert_message,
    clippy::mod_module_files,
    clippy::rest_pat_in_fully_bound_structs,
    clippy::string_slice,
    clippy::suspicious_xor_used_as_pow,
    clippy::tests_outside_test_module,
    clippy::unneeded_field_pattern,
    clippy::verbose_file_reads
)]
use core::time::Duration;

use clap::Parser;

pub mod arg;
pub mod out;
pub mod parser;
pub mod state;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = arg::Args::parse();
    args.init_tracing_subscriber();

    let mut main_state = state::State::new(Duration::from_millis(args.max_wait));
    loop {
        let (output, sleep) = main_state.update()?;
        if let Some(output) = output {
            output.print()?;
        }
        tracing::info!("sleeping for {:?}", sleep);
        std::thread::sleep(sleep);
    }
}
