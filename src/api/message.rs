use std::collections::HashMap;

use actix_web::{error, web, Responder};
use anyhow::Context;
use mongodb::bson::{doc, oid::ObjectId, DateTime};
use serde::Deserialize;

use crate::{
    models::{chat::Chat, chat_message::ChatMessage},
    mongodb::MongoDatabase,
};

use super::{
    user::Claims,
    websocket::{WebsocketMessage, WebsocketState},
};

#[derive(Deserialize)]
struct CreateRequest {
    chat_id: ObjectId,
    content: String,
}

async fn create(
    db: web::Data<MongoDatabase>,
    user: web::ReqData<Claims>,
    ws_state: web::Data<WebsocketState>,
    request: web::Json<CreateRequest>,
) -> actix_web::Result<impl Responder> {
    let chat_collection = db.database.collection::<Chat>("chats");
    let message_collection = db.database.collection::<ChatMessage>("chat_messages");

    let chat = chat_collection
        .find_one(doc! {
            "_id": &request.chat_id
        })
        .await
        .map_err(|err| error::ErrorInternalServerError(err))?
        .context("chat not found")
        .map_err(|err| error::ErrorNotFound(err))?;

    if !chat.user_ids.contains(&user.user.id) {
        return Err(error::ErrorNotFound("chat not found"));
    }

    let ts = DateTime::now();

    let mut message = ChatMessage {
        id: None,
        chat_id: request.chat_id,
        creator: Some(user.user.id),
        created_at: ts,
        deleted_at: None,
        content: request.content.clone(),
    };

    let message_id = message_collection
        .insert_one(&message)
        .await
        .map_err(|err| error::ErrorInternalServerError(err))?;

    message.id = message_id.inserted_id.as_object_id();

    chat_collection
        .update_one(
            doc! {
                "_id": &request.chat_id
            },
            doc! {
                "$set": {
                    "last_message_ts": ts
                }
            },
        )
        .await
        .map_err(|err| error::ErrorInternalServerError(err))?;

    let notif_payload = WebsocketMessage::NewMessage(message.clone().into());

    actix_web::rt::spawn(async move {
        if let Err(err) = ws_state.send_to_users(&chat.user_ids, &notif_payload).await {}
    });

    Ok(web::Json(message))
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/", web::post().to(create));
}
