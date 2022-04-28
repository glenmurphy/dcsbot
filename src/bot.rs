use std::collections::HashMap;

use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::id::{ChannelId, GuildId};
use serenity::prelude::*;
use serenity::http::Http;
use serenity::Client;

use std::sync::atomic::{AtomicBool};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender, UnboundedReceiver};

use crate::dcs::{Servers, ServersMessage};

use serde::{Deserialize, Serialize};
use serde_json;
use std::fs::OpenOptions;
use std::io::BufReader;

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
        let channel = match msg.channel_id.to_channel(&context).await {
            Ok(channel) => channel,
            Err(why) => {
                println!("Error getting channel: {:?}", why);
                return;
            },
        };
        let channel_id = channel.id().0;

        let mut components = msg.content.split(" ");
        match components.next().as_deref() {
            Some("!dcsbot") => {
                match components.next().as_deref() {
                    Some("subscribe") => {
                        println!("Starting DCS bot");
                        let filter = components.next().unwrap_or("");
                        let _ = self.handler_tx.send(BotMessage::SubscribeChannel(channel_id, filter.to_string()));
                    },
                    Some("unsubscribe") => {
                        println!("Stopping DCS bot");
                        let _ = self.handler_tx.send(BotMessage::UnsubscribeChannel(channel_id));
                    },
                    Some(&_) => {},
                    None => {},
                }
            },
            Some(&_) => {},
            None => {},
        }
    }

    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
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
            let o = format!("{} - {}\n{} players online, server address: {}:{}\n\n", 
                sanitize_name(&server.NAME),
                sanitize_name(&server.MISSION_NAME),
                server.PLAYERS.parse::<i32>().unwrap() - 1,
                server.IP_ADDRESS,
                server.PORT);
            output.push(o);
        }
    }

    output.join("")
}

impl Bot {
    pub fn new(token: String, servers_rx: UnboundedReceiver<ServersMessage>) -> Self {
        Bot {
            token,
            servers_rx,
            channels : HashMap::new()
        }
    }

    fn load_channels(&mut self) {
        let open = OpenOptions::new()
            .read(true)
            .open("./channels.json");

        match open {
            Ok(file) => {
                let reader = BufReader::new(file);
                match serde_json::from_reader(reader) {
                    Ok(result) => {
                        self.channels = result;
                        println!("{} channels loaded", self.channels.len());
                    },
                    Err(_) => { println!("Error loading channels"); },
                }
            },
            Err(_) => { println!("No channel file present"); },
        };
    }

    pub async fn connect(&mut self) {
        let intents = GatewayIntents::GUILD_MESSAGES
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

        self.load_channels();
        let http = &Http::new(&self.token);
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .open("./channels.json").unwrap();

        loop {
            tokio::select! {
                Some(servers_message) = self.servers_rx.recv() => {
                    match servers_message {
                        ServersMessage::Servers(servers) => {
                            for (channel_id, sub) in self.channels.iter_mut() {
                                let content = render_servers(&servers, &sub.filter);

                                if content.eq(&sub.last_content) {
                                    continue;
                                }

                                ChannelId(*channel_id).edit_message(http, sub.message_id, |m| m.content(content.clone())).await.unwrap();
                                sub.last_content = content;
                            }
                        }
                    }
                }
                Some(handler_message) = handler_rx.recv() => {
                    match handler_message {
                        BotMessage::SubscribeChannel(channel_id, filter) => {
                            println!("\x1b[32mSubscribing to channel {}\x1b[0m", channel_id);
                            let content = "hello; server listing is being prepared";
                            let message = ChannelId(channel_id).say(http, content).await.unwrap();
                            
                            let sub = Sub {
                                message_id: message.id.0,
                                filter,
                                last_content: content.to_string(),
                            };
                            self.channels.insert(channel_id, sub);
                            match serde_json::to_writer(&file, &self.channels) {
                                Ok(_) => { println!("Channels saved") },
                                Err(_) => { println!("Error saving channels"); },
                            }
                        },
                        BotMessage::UnsubscribeChannel(channel_id) => {
                            println!("\x1b[32mUnsubscribing from channel {}\x1b[0m", channel_id);
                            if !self.channels.contains_key(&channel_id) {
                                return
                            }

                            let message_id = self.channels.get(&channel_id).unwrap().message_id;
                            ChannelId(channel_id).delete_message(http, message_id).await.unwrap();
                            self.channels.remove(&channel_id);
                        },
                    }
                }
            }
        }
    }
}