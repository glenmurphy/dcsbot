## dcsbot
![screenshot](./screenshot.png)

Discord server monitoring bot for [Digital Combat Simulator](https://digitalcombatsimulator.com/)

## Discord Usage

If you a channel manager on a server with dcsbot, you can use the following commands:

```
!dcsbot subscribe <filter>
!dcsbot unsubscribe
```

dcsbot will post a message and keep that message updated (hover over the 'edited' text to see the last time something changed); this works best if DCSBot is in a channel where only it can post messages, which will prevent its message from being pushed off the screen.

## Bot Provider Usage

If you want to run your own bot instead of using the publicly provided one, you will need to create a Discord bot on the [Discord Developer Portal](https://discord.com/developers/applications). Make note of the bot token.

Download the latest dcsbot executable from the [releases](https://github.com/glenmurphy/dcsmon/releases) page

Use your DCS username and password as well as your Discord bot token

    ./dcsbot -u username -p password -t token

To add your DCS bot to your server, you need to create an invitation link by going to the Discord Developer > OAuth2 > URL Generator page and selecting the 'bot' scope followed by the 'send messages', 'manage messages', and 'read message history' permissions. Then visit the link generated at the bottom of the page.

Other options may be added later, see them with

    ./dcsbot --help

## Develop

Requires [Rust](https://www.rust-lang.org/tools/install)

    git clone https://github.com/glenmurphy/dcsbot.git
    cd dcsbot
    cargo run -- -u username -p password -t token
    cargo build --release

The last command will create dcsbot.exe in your dcsmon/target/release directory - move it to whereever you wish.

On non-Windows systems, you may need to install libssl-dev. On Ubuntu you can do that with:

    sudo apt-get install libssl-dev