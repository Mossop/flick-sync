use clap::Parser;
use flexi_logger::Logger;

mod console;

use crate::console::Console;

#[derive(Parser)]
#[clap(author, version)]
struct Args {}

#[tokio::main]
async fn main() -> Result<(), String> {
    let _args = Args::parse();

    let console = Console::new();

    if let Err(e) = Logger::try_with_env_or_str("trace")
        .and_then(|logger| logger.log_to_writer(Box::new(console.clone())).start())
    {
        console.println(format!("Warning, failed to start logging: {}", e));
    }

    Ok(())
}
