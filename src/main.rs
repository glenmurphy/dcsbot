use tokio::sync::mpsc::{unbounded_channel};

use clap::Parser;
mod dcs;
mod bot;

/**
 * Config for clap's command line argument thingy
 */
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Your DCS username (required)
    #[clap(short)]
    username: String,

    /// Your DCS password (required)
    #[clap(short)]
    password: String,
   
    /// Discord bot token
    #[clap(short)]
    token: String,

    /// Config file location
    #[clap(short, default_value = "")]
    filepath: String
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let (servers_tx, servers_rx) = unbounded_channel();

    dcs::start(args.username, args.password, servers_tx).await; // will spawn into background
    bot::start(args.token, args.filepath, servers_rx).await;    // will run main event loop

    Ok(())
}