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

/**
 * We don't really need async for this, especially with the blocking library available,
 * but it's nice to have it for the future (if we want to display progress), and it 
 * doesn't impact the binary size
 */
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let (servers_tx, servers_rx) = unbounded_channel();
    
    //let mut hook = hook::Hook::new(args.filter);

    tokio::spawn(async move {
        dcs::main(args.username, args.password, servers_tx).await;
    });

    let mut bot = bot::Bot::new(args.token, args.filepath, servers_rx);
    bot.connect().await;

    Ok(())
}