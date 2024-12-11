use serenity::async_trait;
use serenity::model::channel::{GuildChannel, Message};
use serenity::model::gateway::Ready;
use serenity::model::id::GuildId;
use serenity::model::permissions::Permissions;
use serenity::model::user::User;
use serenity::prelude::*;
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug)]
pub enum HandlerMessage {
    SubscribeChannel(u64, String), // channel_id, filter
    UnsubscribeChannel(u64),
}

pub struct Handler {
    pub handler_tx: UnboundedSender<HandlerMessage>,
}

fn is_authorized_user(
    channel: GuildChannel,
    cache: &std::sync::Arc<serenity::cache::Cache>,
    author: &User,
) -> bool {
    match channel.permissions_for_user(cache, author) {
        Ok(perm) => return perm.contains(Permissions::MANAGE_CHANNELS),
        Err(err) => println!("Error getting permissions: {:?}", err),
    }
    false
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
            None => {
                println!("Error getting channel");
                return;
            }
        };

        let channel_id = channel.id.0;

        if !is_authorized_user(channel, &context.cache, &msg.author) {
            println!("User was not an admin");
            let _ = msg
                .channel_id
                .say(&context.http, "Sorry I only obey channel managers")
                .await;
            return;
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
                    let _ = self.handler_tx.send(HandlerMessage::SubscribeChannel(
                        channel_id,
                        filter_text.to_string(),
                    ));
                } else {
                    let _ = msg
                        .channel_id
                        .say(
                            &context.http,
                            "Search filter missing. e.g. `!dcsbot subscribe australia`",
                        )
                        .await;
                }
            }
            Some("unsubscribe") => {
                let _ = self
                    .handler_tx
                    .send(HandlerMessage::UnsubscribeChannel(channel_id));
            }
            Some(&_) => {}
            None => {
                let _ = msg
                    .channel_id
                    .say(
                        &context.http,
                        "dcsbot commands: ```!dcsbot subscribe <filter>\n!dcsbot unsubscribe```",
                    )
                    .await;
            }
        }
    }

    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} connected", ready.user.name);
    }

    async fn cache_ready(&self, _: Context, _guilds: Vec<GuildId>) {
        println!("cache ready");
    }
}
