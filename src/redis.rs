use std::{env, pin::pin};

use futures_util::{
    future::{select, Either},
    StreamExt,
};
use mongodb::bson::oid::ObjectId;
use redis::{AsyncCommands, Client};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{self, unbounded_channel};

use crate::api::websocket::{WebsocketServerMessage, WebsocketSeverHandle};

pub struct RedisHandler {
    client: Client,
    ws_server: WebsocketSeverHandle,
    msg_rx: mpsc::UnboundedReceiver<RedisSyncMessage>,
}

#[derive(Clone)]
pub struct RedisHandle {
    msg_tx: mpsc::UnboundedSender<RedisSyncMessage>,
}

impl RedisHandle {
    pub async fn send_message_to_users(
        &self,
        user_ids: &Vec<ObjectId>,
        message: WebsocketServerMessage,
    ) {
        let _ = self.msg_tx.send(RedisSyncMessage {
            user_ids: user_ids.clone(),
            message,
        });
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RedisSyncMessage {
    pub user_ids: Vec<ObjectId>,
    pub message: WebsocketServerMessage,
}

impl RedisHandler {
    pub fn new(ws_server: WebsocketSeverHandle) -> anyhow::Result<(Self, RedisHandle)> {
        let (msg_tx, msg_rx) = unbounded_channel();

        let client = redis::Client::open(env::var("REDIS_URL")?)?;

        Ok((
            Self {
                client,
                ws_server,
                msg_rx,
            },
            RedisHandle { msg_tx },
        ))
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let (mut sink, mut stream) = self.client.get_async_pubsub().await?.split();
        let mut con = self.client.get_multiplexed_async_connection().await?;

        sink.subscribe("sync_messages").await?;

        loop {
            let mut rx = pin!(stream.next());
            let msg_rx = pin!(self.msg_rx.recv());

            match select(msg_rx, rx).await {
                Either::Right((msg, _)) => {}

                Either::Left((Some(msg), _)) => {
                    // send to self
                    self.ws_server
                        .send_message_to_users(&msg.user_ids, msg.message.clone())
                        .await;

                    if let Ok(bytes) = bincode::serialize(&msg) {
                        let _ = con.publish::<_, _, String>("sync_messages", bytes).await;
                    }
                }

                _ => {}
            }
        }
    }
}
