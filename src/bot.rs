use std::collections::HashMap;

use serenity::model::id::{ChannelId};
use serenity::prelude::*;
use serenity::http::Http;
use serenity::Client;

use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use crate::dcs::{Servers, ServersMessage};
use crate::handler::{Handler, HandlerMessage};

use serde::{Deserialize, Serialize};
use serde_json;
use std::fs::{OpenOptions};
use std::io::{BufReader, Result};

#[derive(Serialize, Deserialize)]
pub struct Sub {
    pub message_id: u64,
    pub filter: String,
    pub last_content: String,
}

pub struct Bot {
    pub token : String,
    pub servers_rx: UnboundedReceiver<ServersMessage>,
    pub config_path: String,
    pub channels : HashMap<u64, Sub> // channel_id : message_id mappings
}

/**
 * Turns the DCS goobledegook into something usable in a terminal; won't yet correct
 * for the spaces DCS adds to allow line breaks on its website
 */
fn sanitize_name(name: &str) -> String {
    let mut fixed = name.replace(|c: char| !c.is_ascii(), "");
    fixed = fixed.replace("&amp;", "&");
    fixed = fixed.replace("&gt;", ">");
    fixed = fixed.replace("&lt;", "<");

    if Some(20) == fixed.find(" ") {
        fixed = fixed.replacen(" ", "", 1);
    }
    fixed.trim().to_string()
}

fn render_servers(servers: &Servers, filter : &String) -> String {
    let mut output = vec![];

    for server in &servers.SERVERS {
        if server.NAME.to_lowercase().contains(filter) {
            let o = format!("**{} - {}**\n{} players online, server address: {}:{}, version: {}\n\n", 
                sanitize_name(&server.NAME),
                sanitize_name(&server.MISSION_NAME),
                server.PLAYERS.parse::<i32>().unwrap() - 1,
                server.IP_ADDRESS,
                server.PORT,
                server.DCS_VERSION);
            output.push(o);
        }

        if output.len() > 10 {
            break;
        }
    }

    // Crop output to discord limits
    let string = output.join("");
    if string.len() > 2000 {
        return string.split_at(2000).0.to_string()
     }
     string
}

impl Bot {
    pub fn new(token: String, mut config_path: String, servers_rx: UnboundedReceiver<ServersMessage>) -> Self {
        if config_path.eq("") {
            config_path = "config.json".to_string();
        }
        
        Bot {
            token,
            servers_rx,
            config_path,
            channels : HashMap::new()
        }
    }

    async fn subscribe_channel(&mut self, http: &Http, channel_id: u64, filter: String) {
        println!("\x1b[32mSubscribing to channel {}\x1b[0m", channel_id);

        let content = format!(
            "Server listing with filter '{}' is being prepared...\n\n\
             Server details will be continuously updated in this message (usually within one minute)\n\n\
             To stop receiving updates, delete this message or type !dcsbot unsubscribe", 
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
            Err(err) => println!("Error sending message: {:?}", err)
        }
    }

    async fn unsubscribe_channel(&mut self, http: &Http, channel_id: u64) {
        println!("\x1b[32mUnsubscribing from channel {}\x1b[0m", channel_id);
        if !self.channels.contains_key(&channel_id) {
            return
        }

        let message_id = self.channels.get(&channel_id).unwrap().message_id;
        let _ = ChannelId(channel_id).delete_message(http, message_id).await;
        self.channels.remove(&channel_id);
    }

    async fn broadcast_servers(&mut self, http: &Http, servers: &Servers) -> Result<()> {
        let mut unsubscribe = vec![];

        for (channel_id, sub) in self.channels.iter_mut() {
            let content = render_servers(&servers, &sub.filter);

            if content.eq(&sub.last_content) {
                continue;
            }

            match ChannelId(*channel_id).edit_message(http, sub.message_id, |m| m.content(content.clone())).await {
                Ok(_) => { sub.last_content = content; },
                Err(_) => {
                    // channel_id or message_id might be invalid; unsubscribe
                    println!("\x1b[31mError editing message {} in channel {}\x1b[0m", sub.message_id, channel_id);
                    unsubscribe.push(*channel_id);
                    continue;
                }
            }
        }

        if !unsubscribe.is_empty() {
            for channel_id in &unsubscribe {
                self.unsubscribe_channel(http, *channel_id).await;
            }
            let _ = self.save_channels().await;
        }

        Ok(())
    }

    fn load_channels(&mut self) -> Result<()> {
        let file = OpenOptions::new().read(true).open(self.config_path.clone())?;
        let reader = BufReader::new(file);
        
        self.channels = serde_json::from_reader(reader)?;
        println!("{} channels loaded", self.channels.len());
        Ok(())
    }

    async fn save_channels(&self) -> Result<()> {
        let file = OpenOptions::new().truncate(true).write(true).create(true).open(self.config_path.clone()).unwrap();
        serde_json::to_writer(file, &self.channels)?;
        Ok(())
    }

    async fn event_loop(&mut self, mut handler_rx: UnboundedReceiver<HandlerMessage>) {
        let http = &Http::new(&self.token);
        loop {
            tokio::select! {
                Some(servers_message) = self.servers_rx.recv() => {
                    match servers_message {
                        ServersMessage::Servers(servers) => {
                            let _ = self.broadcast_servers(http, &servers).await;
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
        let intents = GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::GUILDS
            | GatewayIntents::GUILD_MEMBERS
            | GatewayIntents::GUILD_PRESENCES // required for understanding membership information
            | GatewayIntents::DIRECT_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT;

        let  (handler_tx, handler_rx) = unbounded_channel();

        let mut client = Client::builder(self.token.clone(), intents)
            .event_handler(Handler { 
                handler_tx
            })
            .await
            .expect("Error creating client");

        tokio::spawn(async move {
            if let Err(why) = client.start().await {
                println!("An error occurred while running the client: {:?}", why);
            }
        });

        if let Err(msg) = self.load_channels() {
            println!("Error loading channels: {}", msg);
        }

        self.event_loop(handler_rx).await;
    }
}

pub async fn start(token: String, config_path: String, servers_rx: UnboundedReceiver<ServersMessage>) {
    let mut bot = Bot::new(token, config_path, servers_rx);
    bot.start().await;
}