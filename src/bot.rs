use serde::{Deserialize, Serialize};
use serde_json;
use serenity::http::error::Error::UnsuccessfulRequest;
use serenity::http::Http;
use serenity::model::id::ChannelId;
use serenity::prelude::GatewayIntents;
use serenity::Client;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufReader, Result};
use tokio::sync::mpsc;

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
            version_beta: String::new(),
            version_stable: String::new(),
            config_path,
            channels: HashMap::new(),
        }
    }
    /**
     * Turns the DCS-provided strings into something usable
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
    // better with static strings
    fn format_players(&self, players: &str) -> String {
        match players.parse::<i32>().unwrap() - 1 {
            0 => String::from("0 players"),
            1 => String::from("__1 player__"),
            x => format!("__{} players__", x),
        }
    }

    // These format functions are probably slow, and might be made
    // better with static strings
    fn format_version(&self, version: &str) -> String {
        if version.eq(&self.version_beta) {
            return format!("Open Beta ({})", version);
        }
        if version.eq(&self.version_stable) {
            return format!("Stable ({})", version);
        }
        String::from(version)
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
                {}, {}, {}:{}\n\n",
                self.sanitize_name(&server.NAME),
                self.sanitize_name(&server.MISSION_NAME),
                self.format_players(&server.PLAYERS),
                self.format_version(&server.DCS_VERSION),
                server.IP_ADDRESS,
                server.PORT,
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

    /**
     * Subscribes to a channel - will create a message in that channel to post to; if
     * that is unsuccessful, the subscribe will fail, otherwise we will track the
     * channel_id/message_id/filter
     */
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

    /**
     * Unsubscribes from a channel - will attempt to delete the status message we had
     * in that channel
     */
    async fn unsubscribe_channel(&mut self, http: &Http, channel_id: u64) {
        println!("\x1b[32mUnsubscribing from channel {}\x1b[0m", channel_id);
        if !self.channels.contains_key(&channel_id) {
            return;
        }

        let message_id = self.channels.get(&channel_id).unwrap().message_id;
        let _ = ChannelId(channel_id).delete_message(http, message_id).await;
        self.channels.remove(&channel_id);
    }

    fn handle_broadcast_error(&self, err: serenity::Error, message_id: u64, channel_id: u64) -> Option<u64> {
        // Do this here so it's before the err borrow
        let error_text = format!("\x1b[31mError editing message {} in channel {}: {:?}\x1b[0m",
            message_id, channel_id, err);

        match err {
            serenity::Error::Http(http_err) => {
                // TODO: Handle channel-not-found messages
                if let UnsuccessfulRequest(req) = *http_err {
                    if req.error.code == 10008 {
                        println!("\x1b[31mBroadcast Error: Message {} not found in channel {}\x1b[0m", message_id, channel_id);
                        return Some(channel_id);
                    } else if req.error.code == 10003 || req.error.code == 50001 {
                        println!("\x1b[31mBroadcast Error: Channel {} not found\x1b[0m", channel_id);
                        return Some(channel_id);
                    }
                }
            }
            _ => {}
        }

        println!("{}", error_text);
        return None
    }

    /**
     * Go through all subscribed channels/message_ids and update the messages with
     * the current server status. Will unsubscribe from any channel where the update
     * fails (usually because the message was deleted or we lost posting permissions)
     *
     * TODO: Consider messaging server owner on unsubscribe
     */
    async fn broadcast_servers(&mut self, http: &Http, servers: &Servers) -> Result<()> {
        let mut unsubscribe_list = vec![];

        for (channel_id, sub) in self.channels.clone().iter_mut() {
            // Get the text we went to send for this channel
            let content = self.render_servers(&servers, &sub.filter);

            // If it's the same as last time, abort
            // TODO: consider sending anyway after N minutes so the edited time
            // can be bumped (in case people use that to determine staleness)
            if content.eq(&sub.last_content) {
                continue;
            }

            // Send the message and handle any errors; if the message is not found,
            // add it to the unsubscribe list
            match ChannelId(*channel_id)
                .edit_message(http, sub.message_id, |m| m.content(content.clone()))
                .await
            {
                Ok(_) => sub.last_content = content,
                Err(err) => {
                    if let Some(unsubcribe_channel) = self.handle_broadcast_error(err, sub.message_id, *channel_id) {
                        unsubscribe_list.push(unsubcribe_channel);
                    }
                }
            }
        }

        // Unsubscribe from any channels where we couldn't find the message
        if !unsubscribe_list.is_empty() {
            for channel_id in &unsubscribe_list {
                 self.unsubscribe_channel(http, *channel_id).await;
            }
            let _ = self.save_channels().await;
        }

        Ok(())
    }

    /**
     * Load stored channel subscriptions from our file on disk
     */
    fn load_channels(&mut self) -> Result<()> {
        let file = OpenOptions::new()
            .read(true)
            .open(self.config_path.clone())?;
        let reader = BufReader::new(file);

        self.channels = serde_json::from_reader(reader)?;
        println!("{} channels loaded", self.channels.len());
        Ok(())
    }

    /**
     * Load stored channel subscriptions from our file on disk
     * TODO: This might block the rest of the app; consider whether this
     * should run in its own watch-channel-powered thread
     */
    async fn save_channels(&self) -> Result<()> {
        let file = OpenOptions::new()
            .truncate(true)
            .write(true)
            .create(true)
            .open(self.config_path.clone())
            .unwrap();
        serde_json::to_writer(file, &self.channels)?;
        Ok(())
    }

    /**
     * Update the known version strings for Open Beta and Stable
     */
    fn set_versions(&mut self, beta: String, stable: String) {
        println!("Updating versions. beta: {}, stable: {}", beta, stable);
        self.version_beta = beta;
        self.version_stable = stable;
    }

    /**
     * Core event loop for the bot - will listen to messages from the dcs and handler modules
     */
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
