use std::{env, pin::pin};

use actix_web::{rt::signal, web};
use futures_util::{
    future::{select, Either},
    StreamExt,
};
use mongodb::bson::oid::ObjectId;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use shared::api::websocket::WebsocketServerMessage;
use tokio::sync::mpsc::{self, unbounded_channel};

use crate::api::websocket::WebsocketSeverHandle;

pub struct RedisHandler {
    ws_server: web::Data<WebsocketSeverHandle>,
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
    pub fn new(ws_server: web::Data<WebsocketSeverHandle>) -> anyhow::Result<(Self, RedisHandle)> {
        let (msg_tx, msg_rx) = unbounded_channel();

        Ok((Self { ws_server, msg_rx }, RedisHandle { msg_tx }))
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let client = redis::Client::open(env::var("REDIS_URL")?)?;
        let (mut sink, mut stream) = client.get_async_pubsub().await?.split();
        let mut con = client.get_multiplexed_async_connection().await?;

        sink.subscribe("sync_messages").await?;

        loop {
            let rx = pin!(stream.next());
            let msg_rx = pin!(self.msg_rx.recv());
            let close = pin!(signal::ctrl_c());

            let s1 = select(rx, close);

            match select(msg_rx, s1).await {
                Either::Right((Either::Left((Some(msg), _)), _)) => {
                    if let Ok(bytes) = msg.get_payload::<Vec<u8>>() {
                        if let Ok(msg) = bincode::deserialize::<RedisSyncMessage>(&bytes[..]) {
                            self.ws_server
                                .send_message_to_users(&msg.user_ids, msg.message.clone())
                                .await;
                        }
                    }
                }

                Either::Left((Some(msg), _)) => {
                    // send to self
                    self.ws_server
                        .send_message_to_users(&msg.user_ids, msg.message.clone())
                        .await;

                    if let Ok(bytes) = bincode::serialize(&msg) {
                        let _ = con.publish::<_, _, String>("sync_messages", bytes).await;
                    }
                }

                // close signal
                Either::Right((Either::Right(_), _)) => {
                    break;
                }

                // None from rx?
                _ => {}
            }
        }

        Ok(())
    }
}
