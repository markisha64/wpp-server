use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    hash::Hasher,
    sync::Arc,
};

use actix_web::{error, web, HttpRequest, Responder};
use actix_ws::{self, AggregatedMessage, Session};
use futures_util::{lock::Mutex, StreamExt as _};
use uuid::Uuid;

use super::user::Claims;

pub struct WebsocketState {
    connections: Mutex<HashMap<String, HashSet<WsSession>>>,
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
struct WsSession {
    id: Uuid,
    session: Arc<Mutex<Session>>,
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
        .or_insert_with(HashSet::new)
        .insert(ws_session.clone());

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
                            .or_insert(HashSet::new())
                            .remove(&ws_session);

                        return;
                    }
                }
                _ => {
                    let mut mg = ws_state.connections.lock().await;

                    mg.entry(user.user.id.to_string())
                        .or_insert(HashSet::new())
                        .remove(&ws_session);

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
