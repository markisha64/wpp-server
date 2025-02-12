use std::{
    collections::HashMap,
    io,
    pin::pin,
    time::{Duration, Instant},
};

use actix_web::{web, HttpRequest, Responder};
use actix_ws::{self, AggregatedMessage};
use futures_util::{
    future::{select, Either},
    StreamExt as _,
};
use mongodb::bson::oid::ObjectId;
use shared::api::{
    user::Claims,
    websocket::{
        WebsocketClientMessage, WebsocketClientMessageData, WebsocketServerMessage,
        WebsocketServerResData,
    },
};
use uuid::Uuid;

use crate::{mongodb::MongoDatabase, redis::RedisHandle};
use tokio::{
    sync::{mpsc, oneshot},
    time::interval,
};

use super::{
    chat::{self},
    message,
};

type ConnId = Uuid;

enum Command {
    Connect {
        user_id: String,
        conn_tx: mpsc::UnboundedSender<WebsocketServerMessage>,
        res_tx: oneshot::Sender<ConnId>,
    },

    Disconnect {
        user_id: String,
        conn: ConnId,
    },

    Message {
        msg: WebsocketServerMessage,
        user_id: String,
        res_tx: oneshot::Sender<()>,
    },
}

pub struct WebsocketServer {
    connections: HashMap<String, HashMap<Uuid, mpsc::UnboundedSender<WebsocketServerMessage>>>,

    cmd_rx: mpsc::UnboundedReceiver<Command>,

    #[allow(dead_code)]
    db: web::Data<MongoDatabase>,
}

impl WebsocketServer {
    pub fn new(db: web::Data<MongoDatabase>) -> (Self, WebsocketSeverHandle) {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        (
            Self {
                connections: HashMap::new(),
                cmd_rx,
                db: db.clone(),
            },
            WebsocketSeverHandle { cmd_tx, db },
        )
    }

    async fn connect(
        &mut self,
        user_id: String,
        tx: mpsc::UnboundedSender<WebsocketServerMessage>,
    ) -> ConnId {
        let id = Uuid::new_v4();

        self.connections
            .entry(user_id)
            .or_insert_with(HashMap::new)
            .insert(id, tx);

        id
    }

    async fn disconnect(&mut self, user_id: String, conn_id: ConnId) {
        if let Some(conns) = self.connections.get_mut(&user_id) {
            conns.remove(&conn_id);
        }
    }

    async fn send_message(&self, user_id: String, msg: WebsocketServerMessage) {
        if let Some(conns) = self.connections.get(&user_id) {
            for (_, connection) in conns {
                let _ = connection.send(msg.clone());
            }
        }
    }

    pub async fn run(mut self) -> io::Result<()> {
        while let Some(cmd) = self.cmd_rx.recv().await {
            match cmd {
                Command::Connect {
                    conn_tx,
                    res_tx,
                    user_id,
                } => {
                    let conn_id = self.connect(user_id, conn_tx).await;
                    let _ = res_tx.send(conn_id);
                }

                Command::Disconnect { user_id, conn } => {
                    self.disconnect(user_id, conn).await;
                }

                Command::Message {
                    user_id,
                    msg,
                    res_tx,
                } => {
                    self.send_message(user_id, msg).await;
                    let _ = res_tx.send(());
                }
            }
        }

        Ok(())
    }
}

#[derive(Clone)]
pub struct WebsocketSeverHandle {
    cmd_tx: mpsc::UnboundedSender<Command>,

    db: web::Data<MongoDatabase>,
}

impl WebsocketSeverHandle {
    pub async fn connect(
        &self,
        user_id: String,
        conn_tx: mpsc::UnboundedSender<WebsocketServerMessage>,
    ) -> ConnId {
        let (res_tx, res_rx) = oneshot::channel();

        self.cmd_tx
            .send(Command::Connect {
                user_id,
                conn_tx,
                res_tx,
            })
            .unwrap();

        res_rx.await.unwrap()
    }

    pub fn disconnect(&self, user_id: String, conn: ConnId) {
        self.cmd_tx
            .send(Command::Disconnect { user_id, conn })
            .unwrap();
    }

    pub async fn send_message(&self, user_id: String, msg: WebsocketServerMessage) {
        let (res_tx, res_rx) = oneshot::channel();

        self.cmd_tx
            .send(Command::Message {
                msg,
                user_id,
                res_tx,
            })
            .unwrap();

        res_rx.await.unwrap();
    }

    pub async fn send_message_to_users(
        &self,
        user_ids: &Vec<ObjectId>,
        msg: WebsocketServerMessage,
    ) {
        for user_id in user_ids {
            self.send_message(user_id.to_string(), msg.clone()).await;
        }
    }
}

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

fn to_request_response(
    res: anyhow::Result<WebsocketServerResData>,
    id: Uuid,
) -> WebsocketServerMessage {
    match res {
        Ok(data) => WebsocketServerMessage::RequestResponse {
            id,
            error: None,
            data: Some(data),
        },
        Err(err) => WebsocketServerMessage::RequestResponse {
            id,
            error: Some(format!("{}", err)),
            data: None,
        },
    }
}

async fn request_handler(
    request: WebsocketClientMessage,
    ws_server: web::Data<WebsocketSeverHandle>,
    redis_handle: web::Data<RedisHandle>,
    user: &web::ReqData<Claims>,
) -> WebsocketServerMessage {
    let ws_server = ws_server.clone();

    match request.data {
        WebsocketClientMessageData::CreateChat(req_data) => {
            let req_res = chat::create(ws_server.db.clone(), &user, req_data)
                .await
                .map(|data| WebsocketServerResData::CreateChat(data));

            to_request_response(req_res, request.id)
        }

        WebsocketClientMessageData::JoinChat(id) => {
            let req_res = chat::join(ws_server.db.clone(), &user, id)
                .await
                .map(|data| WebsocketServerResData::JoinChat(data));

            to_request_response(req_res, request.id)
        }

        WebsocketClientMessageData::NewMessage(req_data) => {
            let req_res =
                message::create(ws_server.db.clone(), &user, redis_handle.clone(), req_data)
                    .await
                    .map(|data| WebsocketServerResData::NewMessage(data));

            to_request_response(req_res, request.id)
        }

        WebsocketClientMessageData::GetChats => {
            let req_res = chat::get_chats(ws_server.db.clone(), &user)
                .await
                .map(|data| WebsocketServerResData::GetChats(data));

            to_request_response(req_res, request.id)
        }
    }
}

async fn websocket(
    req: HttpRequest,
    body: web::Payload,
    user: web::ReqData<Claims>,
    ws_server: web::Data<WebsocketSeverHandle>,
    redis_handle: web::Data<RedisHandle>,
) -> actix_web::Result<impl Responder> {
    let (res, mut session, msg_stream) = actix_ws::handle(&req, body)?;

    actix_web::rt::spawn(async move {
        let user = user.clone();

        let user_id = user.user.id;

        let mut last_heartbeat = Instant::now();
        let mut interval = interval(HEARTBEAT_INTERVAL);

        let (conn_tx, mut conn_rx) = mpsc::unbounded_channel();

        let conn_id = ws_server.connect(user_id.to_string(), conn_tx).await;

        let msg_stream_f = msg_stream
            .max_frame_size(128 * 1024)
            .aggregate_continuations()
            .max_continuation_size(2 * 1024 * 1024);

        let mut msg_stream = pin!(msg_stream_f);

        let close_reason = loop {
            let tick = pin!(interval.tick());
            let msg_rx = pin!(conn_rx.recv());

            let messages = pin!(select(msg_stream.next(), msg_rx));

            match select(messages, tick).await {
                // commands & messages received from client
                Either::Left((Either::Left((Some(Ok(msg)), _)), _)) => match msg {
                    AggregatedMessage::Ping(bytes) => {
                        last_heartbeat = Instant::now();

                        session.pong(&bytes).await.unwrap();
                    }

                    AggregatedMessage::Pong(_) => {
                        last_heartbeat = Instant::now();
                    }

                    AggregatedMessage::Close(reason) => break reason,

                    AggregatedMessage::Text(payload) => {
                        last_heartbeat = Instant::now();

                        if let Ok(request) = serde_json::from_str::<WebsocketClientMessage>(
                            payload.to_string().as_str(),
                        ) {
                            let res = request_handler(
                                request,
                                ws_server.clone(),
                                redis_handle.clone(),
                                &user,
                            )
                            .await;

                            if let Ok(string_payload) = serde_json::to_string(&res) {
                                session.text(string_payload).await.unwrap();
                            }
                        }
                    }

                    AggregatedMessage::Binary(_) => {
                        last_heartbeat = Instant::now();
                    }
                },

                // ws stream error
                Either::Left((Either::Left((Some(Err(_err)), _)), _)) => {
                    break None;
                }

                // ws stream end
                Either::Left((Either::Left((None, _)), _)) => break None,

                Either::Left((Either::Right((Some(ws_msg), _)), _)) => {
                    if let Ok(notif) = serde_json::to_string(&ws_msg) {
                        let _ = session.text(notif).await;
                    }
                }

                Either::Left((Either::Right((None, _)), _)) => unreachable!(
                    "all connection message senders were dropped; ws server may have panicked"
                ),

                Either::Right((_inst, _)) => {
                    if Instant::now().duration_since(last_heartbeat) > CLIENT_TIMEOUT {
                        break None;
                    }

                    let _ = session.ping(b"").await;
                }
            }
        };

        ws_server.disconnect(user_id.to_string(), conn_id);

        let _ = session.close(close_reason).await;
    });

    Ok(res)
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/{notif_fmt}", web::get().to(websocket));
}
