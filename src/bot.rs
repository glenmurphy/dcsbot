use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::id::{ChannelId, GuildId};
use serenity::prelude::*;
use serenity::utils::MessageBuilder;
use serenity::Client;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::UnboundedReceiver;

use crate::dcs::{Servers, ServersMessage};

pub struct Bot {
    pub token : String,
    pub servers : Servers,
    pub servers_rx: UnboundedReceiver<ServersMessage>,
}

pub struct ChannelSub {
    pub message_id: u64,
    pub message_filter: String,
}

struct Handler {
    is_loop_running: AtomicBool,
}

struct ServersData;
impl TypeMapKey for ServersData {
    type Value = Arc<RwLock<Servers>>;
}

struct SubscriptionsData;
impl TypeMapKey for SubscriptionsData {
    type Value = Arc<RwLock<HashMap<u64, ChannelSub>>>;
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, context: Context, msg: Message) {
        if msg.content == "!dcsbot_start" {
            let channel = match msg.channel_id.to_channel(&context).await {
                Ok(channel) => channel,
                Err(why) => {
                    println!("Error getting channel: {:?}", why);
                    return;
                },
            };

            // The message builder allows for creating a message by
            // mentioning users dynamically, pushing "safe" versions of
            // content (such as bolding normalized content), displaying
            // emojis, and more.
            let response = MessageBuilder::new()
                .push("User ")
                //.push(self.bot.server_data)
                .push_bold_safe(&msg.author.name)
                .push(" used the 'ping' command in the ")
                .mention(&channel)
                .push(" channel")
                .build();

            match msg.channel_id.say(&context.http, &response).await {
                Ok(res) => {
                    println!("Successfully sent message: {:?}", res);

                    let message_id = res.id.0;

                    let subs_lock = {
                        let data_read = context.data.read().await;
                        data_read.get::<SubscriptionsData>().expect("Expected SubscriptionsData in TypeMap.").clone()
                    };       
                    {
                        let mut counter = subs_lock.write().await;
                        let entry = counter.entry(msg.channel_id.0).or_insert(ChannelSub {message_id, message_filter: "".to_string()});
                    }
                }
                Err(why) => println!("Error sending message: {:?}", why)
            }
        }
    }

    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
    }

    // https://github.com/serenity-rs/serenity/blob/current/examples/e12_global_data/src/main.rs
    // https://github.com/serenity-rs/serenity/blob/current/examples/e13_parallel_loops/src/main.rs
    // We use the cache_ready event just in case some cache operation is required in whatever use
    // case you have for this.
    async fn cache_ready(&self, ctx: Context, _guilds: Vec<GuildId>) {
        // it's safe to clone Context, but Arc is cheaper for this use case.
        let ctx = Arc::new(ctx);
        if !self.is_loop_running.load(Ordering::Relaxed) {
            // We have to clone the Arc, as it gets moved into the new thread.
            let ctx1 = Arc::clone(&ctx);

            // tokio::spawn creates a new green thread that can run in parallel with the rest of
            // the application.
            tokio::spawn(async move {
                loop {
                    // We clone Context again here, because Arc is owned, so it moves to the
                    // new function.
                    // ctx1.data.read();
                    // log_system_load(Arc::clone(&ctx1)).await;
                    // let servers = self.servers_rx.recv();
                    let servers = {
                        let mut data = ctx1.data.read().await;
                        data.get::<ServersData>().expect("Expected ServersData in TypeMap.").clone()
                    };
                    let subscriptions = {
                        let mut data = ctx1.data.read().await;
                        data.get::<SubscriptionsData>().expect("Expected SubscriptionsData in TypeMap.").clone().read().await
                    };

                    for (channel_id, sub) in subscriptions.iter() {
                        let message_id = sub.message_id;
                        let filter = sub.message_filter;
                        println!("{} {} {}", channel_id, message_id, filter);

                        let message = ChannelId(*channel_id)
                            .send_message(&ctx, |m| {
                                m.embed(|e| {
                                    e.title("Servers")
                                })
                            })
                            .await;
                    }

                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
            });

            // Now that the loop is running, we set the bool to true
            self.is_loop_running.swap(true, Ordering::Relaxed);
        }
    }
}

impl Bot {
    pub fn new(token: String, servers_rx: UnboundedReceiver<ServersMessage>) -> Self {
        Bot {
            token,
            servers_rx,
            servers : Servers { SERVERS : Vec::new() },
        }
    }

    pub async fn connect(&mut self) {
        let intents = GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::DIRECT_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT;

        let mut client = Client::builder(self.token.clone(), intents)
            .event_handler(Handler { 
                is_loop_running : AtomicBool::new(false),
            })
            .await
            .expect("Error creating client");

        tokio::spawn(async move {
            // start listening for events by starting a single shard
            if let Err(why) = client.start().await {
                println!("An error occurred while running the client: {:?}", why);
            }
        });

        loop {
            tokio::select! {
                Some(msg) = self.servers_rx.recv() => {
                    match msg {
                        ServersMessage::Servers(servers) => {
                            self.servers = servers;

                            // Open the data lock in write mode, so keys can be inserted to it.
                            {
                                let mut data = client.data.write().await;
                        
                                // The CommandCounter Value has the following type:
                                // Arc<RwLock<HashMap<String, u64>>>
                                // So, we have to insert the same type to it.
                                data.insert::<ServersData>(Arc::new(RwLock::new(self.servers)));
                            }
                        },
                    }
                }
            }
        }
    }
}