use clap::Parser;
use tokio::sync::mpsc;

mod bot;
mod dcs;
mod handler;

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
    filepath: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let (servers_tx, servers_rx) = mpsc::channel(1);

    tokio::spawn(async move {
        dcs::start(args.username, args.password, servers_tx).await;
    });

    bot::start(args.token, args.filepath, servers_rx).await;

    // Reaching here would be bad; consider notifying
    println!("Exiting");

    Ok(())
}
