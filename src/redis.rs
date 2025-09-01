use std::{env, pin::pin, time::Duration};

use actix_web::{rt::signal, web};
use anyhow::anyhow;
use futures_util::{
    future::{select, Either},
    StreamExt,
};
use mongodb::bson::oid::ObjectId;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use shared::api::websocket::WebsocketServerMessage;
use tokio::{
    sync::{
        mpsc::{self, unbounded_channel},
        oneshot,
    },
    time::sleep,
};
use tracing::{error, info, warn};

use crate::api::websocket::WebsocketServerHandle;

pub struct RedisHandler {
    ws_server: web::Data<WebsocketServerHandle>,
    msg_rx: mpsc::UnboundedReceiver<RedisCommand>,
}

#[derive(Clone)]
pub struct RedisHandle {
    msg_tx: mpsc::UnboundedSender<RedisCommand>,
}

impl RedisHandle {
    pub async fn send_message_to_users(
        &self,
        user_ids: &Vec<ObjectId>,
        message: WebsocketServerMessage,
    ) {
        let _ = self.msg_tx.send(RedisCommand::Send(RedisSyncMessage {
            user_ids: user_ids.clone(),
            message,
        }));
    }

    pub fn commit_server(&self, ip: String, port: String, room_id: String) {
        let _ = self
            .msg_tx
            .send(RedisCommand::CommitServer { ip, room_id, port });
    }

    pub fn remove_server(&self, ip: String, port: String, room_id: String) {
        let _ = self
            .msg_tx
            .send(RedisCommand::RemoveServer { ip, room_id, port });
    }

    pub async fn get_servers(&self, room_id: String) -> anyhow::Result<Vec<(String, String)>> {
        let (res_tx, res_rx) = oneshot::channel();

        self.msg_tx
            .send(RedisCommand::GetServers { room_id, res_tx })?;

        Ok(res_rx.await?)
    }
}

pub enum RedisCommand {
    Send(RedisSyncMessage),
    GetServers {
        room_id: String,
        res_tx: oneshot::Sender<Vec<(String, String)>>,
    },
    CommitServer {
        ip: String,
        room_id: String,
        port: String,
    },
    RemoveServer {
        ip: String,
        room_id: String,
        port: String,
    },
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RedisSyncMessage {
    pub user_ids: Vec<ObjectId>,
    pub message: WebsocketServerMessage,
}

impl RedisHandler {
    pub fn new(ws_server: web::Data<WebsocketServerHandle>) -> anyhow::Result<(Self, RedisHandle)> {
        let (msg_tx, msg_rx) = unbounded_channel();

        Ok((Self { ws_server, msg_rx }, RedisHandle { msg_tx }))
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let mut backoff_duration = Duration::from_secs(1);
        let max_backoff = Duration::from_secs(60);
        let mut consecutive_failures = 0;
        let max_consecutive_failures = 5;

        loop {
            match self.run_connection().await {
                Ok(_) => {
                    info!("Gracefully shutting down RedisHandler");
                    break;
                }
                Err(e) => {
                    consecutive_failures += 1;

                    if consecutive_failures >= max_consecutive_failures {
                        error!(
                            "Too many consecutive Redis connection failures ({}), giving up",
                            consecutive_failures
                        );
                        return Err(e);
                    }

                    warn!(
                        "Redis connection failed (attempt {}): {:?}. Retrying in {:?}",
                        consecutive_failures, e, backoff_duration
                    );

                    sleep(backoff_duration).await;

                    backoff_duration = std::cmp::min(backoff_duration * 2, max_backoff);
                }
            }
        }

        Ok(())
    }

    async fn run_connection(&mut self) -> anyhow::Result<()> {
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
                Either::Right((Either::Left((Some(msg), _)), _)) => match msg.get_channel_name() {
                    "sync_messages" => {
                        if let Ok(payload) = msg.get_payload::<String>() {
                            if let Ok(msg) = serde_json::from_str::<RedisSyncMessage>(&payload) {
                                self.ws_server
                                    .send_message_to_users(&msg.user_ids, msg.message.clone())
                                    .await;
                            }
                        }
                    }

                    cname => {
                        error!("invalid message \"{cname}\": {:?}", msg);
                    }
                },

                Either::Left((Some(command), _)) => match command {
                    RedisCommand::Send(msg) => {
                        if let Ok(payload) = serde_json::to_string(&msg) {
                            let _ = con.publish::<_, _, String>("sync_messages", payload).await;
                        }
                    }

                    RedisCommand::CommitServer { ip, room_id, port } => {
                        let _ = con.sadd::<_, _, i32>(room_id, (ip, port)).await;
                    }

                    RedisCommand::RemoveServer { ip, room_id, port } => {
                        let _ = con.srem::<_, _, i32>(room_id, (ip, port)).await;
                    }

                    RedisCommand::GetServers { room_id, res_tx } => {
                        if let Ok(members) = con.smembers::<_, Vec<(String, String)>>(room_id).await
                        {
                            let _ = res_tx.send(members);
                        }
                    }
                },

                // msg_rx None, server died?
                Either::Left((None, _)) => {
                    panic!("msg_rx None, server shutting down")
                }

                // close signal
                Either::Right((Either::Right((evt, _)), _)) => {
                    evt.expect("failed to listen for ctrl-c");

                    return Ok(());
                }

                // None from rx, lost connection?
                Either::Right((Either::Left((None, _)), _)) => {
                    return Err(anyhow!("Redis connection lost"));
                }
            }
        }
    }
}
