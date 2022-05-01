use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufReader, Result};

use tokio::sync::mpsc;

use serde::{Deserialize, Serialize};
use serde_json;

use serenity::http::Http;
use serenity::model::id::ChannelId;
use serenity::prelude::GatewayIntents;
use serenity::Client;

use crate::dcs::{Servers, ServersMessage};
use crate::handler::{Handler, HandlerMessage};


#[derive(Serialize, Deserialize, Clone)]
pub struct Sub {
    pub message_id: u64,
    pub filter: String,
    pub last_content: String,
}

pub struct Bot {
    pub token: String,
    pub servers_rx: mpsc::Receiver<ServersMessage>,
    version_beta: String,
    version_stable: String,
    pub config_path: String,
    pub channels: HashMap<u64, Sub>, // channel_id : message_id mappings
}

impl Bot {
    pub fn new(
        token: String,
        mut config_path: String,
        servers_rx: mpsc::Receiver<ServersMessage>,
    ) -> Self {
        if config_path.eq("") {
            config_path = "config.json".to_string();
        }

        Bot {
            token,
            servers_rx,
            version_beta : String::new(),
            version_stable : String::new(),
            config_path,
            channels: HashMap::new(),
        }
    }
    /**
     * Turns the DCS goobledegook into something usable
     */
    fn sanitize_name(&self, name: &str) -> String {
        // Get rid of the decorations people use; this will probably
        // mess up non-English names, so need to be more artful here
        let mut fixed = name.replace(|c: char| !c.is_ascii(), "");

        // Convert HTML special chars
        fixed = fixed.replace("&amp;", "&");
        fixed = fixed.replace("&gt;", ">");
        fixed = fixed.replace("&lt;", "<");

        // ED adds spaces to allow linebreaks on the DCS website
        // we can't tell if this is added by them or part of the
        // actual data, so if the server owner intentionally had
        // a space at char 20, this will sadly remove it
        if Some(20) == fixed.find(" ") {
            fixed = fixed.replacen(" ", "", 1);
        }

        fixed.trim().to_string()
    }

    // These format functions are probably slow, and might be made 
    // faster with static strings
    fn format_players(&self, players: &String) -> String {
        match players.parse::<i32>().unwrap() - 1 {
            0 => String::from("0 players"),
            1 => String::from("__1 player__"),
            x => format!("__{} players__", x)
        }
    }

    // These format functions are probably slow, and might be made 
    // faster with static strings
    fn format_version(&self, version: &String) -> String {
        if version.eq(&self.version_beta) {
            return format!("Open Beta ({})", version)
        }
        if version.eq(&self.version_stable) {
            return format!("Stable ({})", version)
        }
        return version.clone()
    }

    /**
     * Takes a list of all the servers, finds the one matching <filter>, and
     * renders the result into Discord-friendly markdown
     */
    fn render_servers(&self, servers: &Servers, filter: &String) -> String {
        let mut output = vec![];
        let mut sorted = vec![];

        for server in &servers.SERVERS {
            if !server.NAME.to_lowercase().contains(filter) {
                continue;
            }
            sorted.push(server);
        }

        // Without semver understanding, this might do all kinds of stuff, but
        // for now it's OK because it will at least group the servers together
        sorted.sort_by_cached_key(|a| a.DCS_VERSION.clone());
        sorted.reverse();

        for server in sorted {
            output.push(format!(
                "**{} - {}**\n\
                {}, server: {}:{}, {}\n\n",
                self.sanitize_name(&server.NAME),
                self.sanitize_name(&server.MISSION_NAME),
                self.format_players(&server.PLAYERS),
                server.IP_ADDRESS,
                server.PORT,
                self.format_version(&server.DCS_VERSION)
            ));

            if output.len() > 10 {
                break;
            }
        }

        // Crop output to discord limits
        let string = output.join("");
        if string.len() > 1999 {
            return string.split_at(1999).0.to_string();
        }
        string
    }

    async fn subscribe_channel(&mut self, http: &Http, channel_id: u64, filter: String) {
        println!("\x1b[32mSubscribing to channel {}\x1b[0m", channel_id);

        let content = format!(
            "Server listing with filter '{}' is being prepared...\n\n\
             Server details will be continuously updated in this message (usually within one minute)\n\n\
             To stop receiving updates, delete this message or type `!dcsbot unsubscribe`", 
            filter);

        match ChannelId(channel_id).say(http, content.clone()).await {
            Ok(message) => {
                let sub = Sub {
                    message_id: message.id.0,
                    filter,
                    last_content: content,
                };
                self.channels.insert(channel_id, sub);
            }
            Err(err) => println!("Error sending message: {:?}", err),
        }
    }

    async fn unsubscribe_channel(&mut self, http: &Http, channel_id: u64) {
        println!("\x1b[32mUnsubscribing from channel {}\x1b[0m", channel_id);
        if !self.channels.contains_key(&channel_id) {
            return;
        }

        let message_id = self.channels.get(&channel_id).unwrap().message_id;
        let _ = ChannelId(channel_id).delete_message(http, message_id).await;
        self.channels.remove(&channel_id);
    }

    async fn broadcast_servers(&mut self, http: &Http, servers: &Servers) -> Result<()> {
        let mut unsubscribe_list = vec![];

        for (channel_id, sub) in self.channels.clone().iter_mut() {
            let content = self.render_servers(&servers, &sub.filter);

            if content.eq(&sub.last_content) {
                continue;
            }

            match ChannelId(*channel_id)
                .edit_message(http, sub.message_id, |m| m.content(content.clone()))
                .await
            {
                Ok(_) => {
                    sub.last_content = content;
                }
                Err(_) => {
                    // channel_id or message_id might be invalid; unsubscribe
                    println!(
                        "\x1b[31mError editing message {} in channel {}\x1b[0m",
                        sub.message_id, channel_id
                    );
                    unsubscribe_list.push(*channel_id);
                    continue;
                }
            }
        }

        if !unsubscribe_list.is_empty() {
            for channel_id in &unsubscribe_list {
                self.unsubscribe_channel(http, *channel_id).await;
            }
            let _ = self.save_channels().await;
        }

        Ok(())
    }

    fn load_channels(&mut self) -> Result<()> {
        let file = OpenOptions::new()
            .read(true)
            .open(self.config_path.clone())?;
        let reader = BufReader::new(file);

        self.channels = serde_json::from_reader(reader)?;
        println!("{} channels loaded", self.channels.len());
        Ok(())
    }

    async fn save_channels(&self) -> Result<()> {
        // This might be a blocking point; consider whether the saving system
        // should run in its own watch-channel thread
        let file = OpenOptions::new()
            .truncate(true)
            .write(true)
            .create(true)
            .open(self.config_path.clone())
            .unwrap();
        serde_json::to_writer(file, &self.channels)?;
        Ok(())
    }

    fn set_versions(&mut self, beta: String, stable: String) {
        println!("Updating versions. beta: {}, stable: {}", beta, stable);
        self.version_beta = beta;
        self.version_stable = stable;
    }

    async fn event_loop(&mut self, mut handler_rx: mpsc::UnboundedReceiver<HandlerMessage>) {
        let http = &Http::new(&self.token);
        loop {
            tokio::select! {
                Some(servers_message) = self.servers_rx.recv() => {
                    match servers_message {
                        ServersMessage::Servers(servers) => {
                            let _ = self.broadcast_servers(http, &servers).await;
                        }
                        ServersMessage::Versions(versions) => {
                            self.set_versions(versions.0, versions.1)
                        }
                    }
                },
                Some(handler_message) = handler_rx.recv() => {
                    match handler_message {
                        HandlerMessage::SubscribeChannel(channel_id, filter) => {
                            self.subscribe_channel(http, channel_id, filter).await;
                            let _ = self.save_channels().await;
                        },
                        HandlerMessage::UnsubscribeChannel(channel_id) => {
                            self.unsubscribe_channel(http, channel_id).await;
                            let _ = self.save_channels().await;
                        },
                    }
                }
            }
        }
    }

    pub async fn start(&mut self) {
        if let Err(msg) = self.load_channels() {
            println!("Error loading channels: {}", msg);
        }

        let intents = GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::GUILDS
            | GatewayIntents::GUILD_MEMBERS
            | GatewayIntents::GUILD_PRESENCES // required to see membership permissions
            | GatewayIntents::DIRECT_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT;

        let (handler_tx, handler_rx) = mpsc::unbounded_channel();

        let mut client = Client::builder(self.token.clone(), intents)
            .event_handler(Handler { handler_tx })
            .await
            .expect("Error creating client");

        tokio::spawn(async move {
            if let Err(why) = client.start().await {
                println!("An error occurred while running the client: {:?}", why);
            }
            // Reaching here would be bad; consider notifying.
            // TODO: figure out the causes of reaching here (e.g. is Serenity 
            // robust against disconnection?)
            println!("Unexpected exit");
        });

        self.event_loop(handler_rx).await;
    }
}

pub async fn start(token: String, config_path: String, servers_rx: mpsc::Receiver<ServersMessage>) {
    let mut bot = Bot::new(token, config_path, servers_rx);
    bot.start().await;
}
