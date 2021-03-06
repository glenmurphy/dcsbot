## dcsbot
![screenshot](./screenshot.png)

Discord server monitoring bot for [Digital Combat Simulator](https://digitalcombatsimulator.com/)

Contact [glen@glenmurphy.com](mailto:glen@glenmurphy.com) if you want to use this on your Discord server

## Discord Usage

If you a channel manager on a server with dcsbot, you can use the following commands:

```
!dcsbot subscribe <filter>
!dcsbot unsubscribe
```

dcsbot will post a message and keep that message updated (hover over the 'edited' text to see the last time something changed); this works best if DCSBot is in a channel where only it can post messages, which will prevent its message from being pushed off the screen.

## Create your own dcsbot

This only matters if you want to run your own dcsbot instead of using the official one

1. You will need to create a Discord bot on the [Discord Developer Portal](https://discord.com/developers/applications). Make note of the bot token.
2. Download the latest dcsbot executable from the [releases](https://github.com/glenmurphy/dcsmon/releases) page
3. Use your DCS username and password as well as your Discord bot token: `./dcsbot -u username -p password -t token`
4. To add your DCS bot to your server, create an invitation link by going to the Discord Developer > OAuth2 > URL Generator page and selecting the 'bot' scope followed by the 'send messages' permission. Then visit the link generated at the bottom of the page.
5. Other options may be added later, see them with `1`./dcsbot --help`

## Develop

Requires [Rust](https://www.rust-lang.org/tools/install)

    git clone https://github.com/glenmurphy/dcsbot.git
    cd dcsbot
    cargo run -- -u username -p password -t token
    cargo build --release

The last command will create dcsbot.exe in your dcsmon/target/release directory - move it to whereever you wish.

On non-Windows systems, you may need to install libssl-dev. On Ubuntu you can do that with:

    sudo apt-get install libssl-dev

## Architecture

- Core Discord functionality is provided by [Serenity](https://github.com/serenity-rs/serenity)
- Many Tokio threads communicating through unbounded_channels
- The **dcs** module polls and parses the server listing provided by the digitalcombatsimulator.com website and sends the results to **bot**
- **bot** listens for discord commands via **handler** - when it gets a valid subscription request, it posts a message to that channel and stores the {channel_id, message_id, and filtertext} as a **Sub** in self.channels (indexed by channel id, so only one active message per channel)
- When **bot** receives the list of servers from **dcs**, it updates each message_id stored **Sub** with the appropriate filtered view
- If the message is deleted by an admin or unsubscribe is called, **bot** will delete the subscription
- Those subs/channels are backed up to the specified config file (config.json by default)
