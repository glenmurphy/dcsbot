## dcsbot
![screenshot](./screenshot.png)

Discord server monitoring bot for [Digital Combat Simulator](https://digitalcombatsimulator.com/)

## User Usage

If you are in a channel with dcsbot, you can use the following commands:

```
!dcsbot subscribe <filter>
!dcsbot unsubscribe
```

## Host Usage

Download the latest dcsbot executable from the [releases](https://github.com/glenmurphy/dcsmon/releases) page

Use your DCS username and password as well as your Discord bot token

    ./dcsbot -u username -p password -t token

Other options may be added later, see them with

    ./dcsbot --help


## Develop

Requires [Rust](https://www.rust-lang.org/tools/install)

    git clone https://github.com/glenmurphy/dcsmon.git
    cd dcsmon
    cargo run -- -u username -p password
    cargo build --release

The last command will create dcsbot.exe in your dcsmon/target/release directory - move it to whereever you wish.