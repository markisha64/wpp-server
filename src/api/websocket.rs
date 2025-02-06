use std::{collections::HashMap, hash::Hash, hash::Hasher, sync::Arc};

use actix_web::{web, HttpRequest, Responder};
use actix_ws::{self, AggregatedMessage, Session};
use futures_util::{lock::Mutex, StreamExt as _};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::chat_message::ChatMessageSafe;

use super::user::Claims;

pub struct WebsocketState {
    pub connections: Mutex<HashMap<String, HashMap<Uuid, WsSession>>>,
}

impl WebsocketState {
    fn new() -> Self {
        Self {
            connections: Mutex::new(HashMap::new()),
        }
    }

    pub fn init() -> web::Data<Self> {
        web::Data::new(Self::new())
    }
}

#[derive(Clone, Debug)]
pub struct WsSession {
    pub id: Uuid,
    pub session: Arc<Mutex<Session>>,
}

impl PartialEq for WsSession {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for WsSession {}

impl Hash for WsSession {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum WebsocketMessage {
    NewMessage(ChatMessageSafe),
}

async fn websocket(
    req: HttpRequest,
    body: web::Payload,
    user: web::ReqData<Claims>,
    ws_state: web::Data<WebsocketState>,
) -> actix_web::Result<impl Responder> {
    let (response, session, msg_stream) = actix_ws::handle(&req, body)?;

    let mut mg = ws_state.connections.lock().await;

    let session = Arc::new(Mutex::new(session));

    let session_uuid = Uuid::new_v4();

    let ws_session = WsSession {
        id: session_uuid,
        session: session.clone(),
    };

    mg.entry(user.user.id.to_string())
        .or_insert_with(HashMap::new)
        .insert(session_uuid, ws_session.clone());

    // need to drop it so it doesn't get moved
    drop(mg);

    let mut stream = msg_stream
        .aggregate_continuations()
        .max_continuation_size(2_usize.pow(20));

    actix_web::rt::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                AggregatedMessage::Ping(msg) => {
                    let mut s = session.lock().await;

                    if s.pong(&msg).await.is_err() {
                        let mut mg = ws_state.connections.lock().await;

                        mg.entry(user.user.id.to_string())
                            .or_insert_with(HashMap::new)
                            .remove(&session_uuid);

                        return;
                    }
                }
                _ => {
                    let mut mg = ws_state.connections.lock().await;

                    mg.entry(user.user.id.to_string())
                        .or_insert_with(HashMap::new)
                        .remove(&session_uuid);

                    break;
                }
            }
        }
    });

    Ok(response)
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/", web::get().to(websocket));
}
