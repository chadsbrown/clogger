mod fakes;
mod runner;
mod script;

use anyhow::{Context, Result};
use runner::run_script_file;

fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .context("usage: logger-cli <script.json>")?;

    run_script_file(&path)?;
    println!("script passed: {path}");
    Ok(())
}
