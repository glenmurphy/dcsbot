use std::collections::HashMap;

use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::id::{ChannelId};
use serenity::prelude::*;
use serenity::http::Http;
use serenity::Client;
use serenity::model::prelude::*;
use serenity::model::permissions::Permissions;
use serenity::model::id::GuildId;

use tokio::sync::mpsc::{unbounded_channel, UnboundedSender, UnboundedReceiver};

use crate::dcs::{Servers, ServersMessage};

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
    pub channels : HashMap<u64, Sub> // channel_id : message_id mappings
}

pub enum BotMessage {
    SubscribeChannel(u64, String), // channel_id, filter
    UnsubscribeChannel(u64),
}

struct Handler {
    handler_tx : UnboundedSender<BotMessage>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, context: Context, msg: Message) {
        let mut components = msg.content.split(" ");
        if components.next().as_deref().unwrap_or_default() != "!dcsbot" {
            return;
        }

        let channel = match context.cache.guild_channel(msg.channel_id) {
            Some(channel) => channel,
            None => { println!("Error getting channel"); return }
        };

        let channel_id = channel.id.0;

        match channel.permissions_for_user(&context.cache, &msg.author) {
            Ok(permissions) => {
                if !permissions.contains(Permissions::MANAGE_CHANNELS) {
                    println!("User was not an admin");
                    let _ = msg.channel_id.say(&context.http, "Sorry I only obey channel managers").await;
                    return;
                } else {
                    println!("User is an admin");
                }
            }
            Err(err) => println!("Error getting permissions: {:?}", err)
        }

        match components.next().as_deref() {
            Some("subscribe") => {
                // Split.as_str() would be nice here
                let mut filter = vec![];
                while let Some(word) = components.next() {
                    filter.push(word);
                }
                if filter.len() > 0 {
                    let filter_text = filter.join(" ");
                    let _ = self.handler_tx.send(BotMessage::SubscribeChannel(channel_id, filter_text.to_string()));
                } else {
                    let _ = msg.channel_id.say(&context.http, "You need to provide a filter. e.g. ```!dcsbot subscribe australia```").await;
                }
            },
            Some("unsubscribe") => {
                let _ = self.handler_tx.send(BotMessage::UnsubscribeChannel(channel_id));
            },
            Some(&_) => {},
            None => {
                let _ = msg.channel_id.say(&context.http, "dcsbot commands: ```!dcsbot subscribe australia\n!dcsbot unsubscribe```").await;
            },
        }
    }

    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} connected", ready.user.name);
    }

    async fn cache_ready(&self, _: Context, _guilds: Vec<GuildId>) {
        println!("Cache ready");
    }
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

    let string = output.join("");
    if string.len() > 2000 {
        string.split_at(2000).0.to_string()
     } else {
        string
     }
}

impl Bot {
    pub fn new(token: String, servers_rx: UnboundedReceiver<ServersMessage>) -> Self {
        Bot {
            token,
            servers_rx,
            channels : HashMap::new()
        }
    }

    async fn subscribe_channel(&mut self, http: &Http, channel_id: u64, filter: String) {
        println!("\x1b[32mSubscribing to channel {}\x1b[0m", channel_id);

        let content = format!(
            "Server listing with filter '{}' is being prepared...\n\n\
             Server details will go here - you may delete any other dcsbot messages in this channel", 
            filter);
        let message = ChannelId(channel_id).say(http, content.clone()).await.unwrap();
        
        let sub = Sub {
            message_id: message.id.0,
            filter,
            last_content: content,
        };
        self.channels.insert(channel_id, sub);
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
        let file = OpenOptions::new().read(true).open("./channels.json")?;
        let reader = BufReader::new(file);
        
        self.channels = serde_json::from_reader(reader)?;
        println!("{} channels loaded", self.channels.len());
        Ok(())
    }

    async fn save_channels(&self) -> Result<()> {
        let file = OpenOptions::new().truncate(true).write(true).create(true).open("./channels.json").unwrap();
        serde_json::to_writer(file, &self.channels)?;
        Ok(())
    }

    pub async fn connect(&mut self) {
        let intents = GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::GUILDS
            | GatewayIntents::GUILD_MEMBERS
            | GatewayIntents::GUILD_PRESENCES // required for understanding membership information
            | GatewayIntents::DIRECT_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT;

        let  (handler_tx, mut handler_rx) = unbounded_channel();

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
                        BotMessage::SubscribeChannel(channel_id, filter) => {
                            self.subscribe_channel(http, channel_id, filter).await;
                            let _ = self.save_channels().await;
                        },
                        BotMessage::UnsubscribeChannel(channel_id) => {
                            self.unsubscribe_channel(http, channel_id).await;
                            let _ = self.save_channels().await;
                        },
                    }
                }
            }
        }
    }
}