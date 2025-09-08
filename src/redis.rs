use std::{env, pin::pin, time::Duration};

use actix_web::{rt::signal, web};
use anyhow::{anyhow, Context};
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

use crate::api::websocket::WebsocketSeverHandle;

pub struct RedisHandler {
    ws_server: web::Data<WebsocketSeverHandle>,
    msg_rx: mpsc::UnboundedReceiver<Command>,
}

#[derive(Clone)]
pub struct RedisHandle {
    msg_tx: mpsc::UnboundedSender<Command>,
}

enum Command {
    Send {
        send: RedisSyncMessage,
        res_tx: oneshot::Sender<anyhow::Result<()>>,
    },
    SinkRoom {
        room_id: ObjectId,
        res_tx: oneshot::Sender<anyhow::Result<bool>>,
    },
    UnsinkRoom {
        room_id: ObjectId,
        res_tx: oneshot::Sender<anyhow::Result<()>>,
    },
}

impl RedisHandle {
    pub async fn send_message_to_users(
        &self,
        user_ids: &Vec<ObjectId>,
        message: WebsocketServerMessage,
    ) -> anyhow::Result<()> {
        let (res_tx, res_rx) = oneshot::channel();

        self.msg_tx.send(Command::Send {
            send: RedisSyncMessage {
                user_ids: user_ids.clone(),
                message,
            },
            res_tx,
        })?;

        res_rx.await?
    }

    pub async fn sink_room(&self, room_id: ObjectId) -> anyhow::Result<bool> {
        let (res_tx, res_rx) = oneshot::channel();

        self.msg_tx.send(Command::SinkRoom { room_id, res_tx })?;

        res_rx.await?
    }

    pub async fn unsink_room(&self, room_id: ObjectId) -> anyhow::Result<()> {
        let (res_tx, res_rx) = oneshot::channel();

        self.msg_tx.send(Command::UnsinkRoom { room_id, res_tx })?;

        res_rx.await?
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
                Either::Right((Either::Left((Some(msg), _)), _)) => {
                    if let Ok(payload) = msg.get_payload::<String>() {
                        if let Ok(msg) = serde_json::from_str::<RedisSyncMessage>(&payload) {
                            self.ws_server
                                .send_message_to_users(&msg.user_ids, msg.message.clone())
                                .await;
                        }
                    }
                }

                Either::Left((Some(cmd), _)) => match cmd {
                    Command::Send { send, res_tx } => {
                        let task: Result<(), anyhow::Error> = async {
                            let payload = serde_json::to_string(&send)?;

                            con.publish::<_, _, String>("sync_messages", payload)
                                .await
                                .map(|_| ())
                                .map_err(|x| anyhow!(x))
                        }
                        .await;

                        let _ = res_tx.send(task);
                    }

                    Command::SinkRoom { room_id, res_tx } => {
                        let task: Result<bool, anyhow::Error> = async {
                            let result: Vec<(String, i64)> = redis::cmd("PUBSUB")
                                .arg("NUMSUB")
                                .arg(room_id.to_string().as_str())
                                .query_async(&mut con)
                                .await?;

                            let available = result
                                .get(0)
                                .map(|(_, x)| *x > 0)
                                .context("empty result set")?;

                            if !available {
                                return Ok(false);
                            }

                            sink.subscribe(room_id.to_string()).await?;

                            Ok(true)
                        }
                        .await;

                        let _ = res_tx.send(task);
                    }

                    Command::UnsinkRoom { room_id, res_tx } => {
                        let _ = res_tx.send(
                            sink.unsubscribe(room_id.to_string())
                                .await
                                .map_err(|x| anyhow!(x)),
                        );
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
