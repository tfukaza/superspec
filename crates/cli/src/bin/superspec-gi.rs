//! Read a WorldSpec probe request on stdin and write linear RGB8 lobes on stdout.
use std::io::{self, Write};
use superspec_engine::worldspec::bounced_light::bake;
use superspec_types::bounced_light::ProbeBake;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let request: ProbeBake = serde_json::from_reader(io::stdin().lock())?;
    io::stdout().lock().write_all(&bake(request)?)?;
    Ok(())
}
