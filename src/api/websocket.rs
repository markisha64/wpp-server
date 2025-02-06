use std::{collections::HashMap, sync::Arc};

use actix_web::{web, HttpRequest, Responder};
use actix_ws::{self, AggregatedMessage, Session};
use futures_util::{lock::Mutex, StreamExt as _};
use mongodb::bson::oid::ObjectId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::chat_message::ChatMessageSafe;

use super::user::Claims;

pub struct WebsocketState {
    pub connections: Mutex<HashMap<String, HashMap<Uuid, Arc<Mutex<Session>>>>>,
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

    pub async fn send_to_users(
        &self,
        user_ids: &Vec<ObjectId>,
        message: &WebsocketMessage,
    ) -> anyhow::Result<()> {
        let notif = serde_json::to_string(message)?;
        let notif_str = notif.as_str();

        for user_id in user_ids {
            let mut mg = self.connections.lock().await;

            let set = mg.entry(user_id.to_string()).or_insert_with(HashMap::new);

            let mut to_remove = Vec::new();

            for (id, connection) in set.iter() {
                let mut session = connection.lock().await;

                if session.text(notif_str).await.is_err() {
                    to_remove.push(*id);
                }
            }

            for id in to_remove.iter() {
                set.remove(id);
            }
        }

        Ok(())
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

    mg.entry(user.user.id.to_string())
        .or_insert_with(HashMap::new)
        .insert(session_uuid, session.clone());

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
