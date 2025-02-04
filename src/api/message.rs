use actix_web::{error, web, Responder};
use anyhow::Context;
use mongodb::bson::{doc, oid::ObjectId, DateTime};
use serde::Deserialize;

use crate::{
    models::{chat::Chat, chat_message::ChatMessage},
    mongodb::MongoDatabase,
};

use super::user::Claims;

#[derive(Deserialize)]
struct CreateRequest {
    chat_id: ObjectId,
    content: String,
}

async fn create(
    db: web::Data<MongoDatabase>,
    user: web::ReqData<Claims>,
    request: web::Json<CreateRequest>,
) -> actix_web::Result<impl Responder> {
    let chat_collection = db.database.collection::<Chat>("chats");
    let message_collection = db.database.collection::<ChatMessage>("messages");

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

    let mut message = ChatMessage {
        id: None,
        chat_id: request.chat_id,
        creator: Some(user.user.id),
        created_at: DateTime::now(),
        deleted_at: None,
        content: request.content.clone(),
    };

    let message_id = message_collection
        .insert_one(&message)
        .await
        .map_err(|err| error::ErrorInternalServerError(err))?;

    message.id = message_id.inserted_id.as_object_id();

    Ok(web::Json(message))
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/", web::post().to(create));
}
