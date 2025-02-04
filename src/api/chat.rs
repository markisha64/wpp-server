use std::future::IntoFuture;

use actix_web::{error, web, Responder};
use anyhow::Context;
use mongodb::bson::{doc, oid::ObjectId, DateTime};
use serde::{Deserialize, Serialize};

use futures_util::{try_join, TryFutureExt};

use crate::{
    models::{chat::Chat, chat_message::ChatMessage},
    mongodb::MongoDatabase,
};

use super::user::Claims;

#[derive(Deserialize)]
struct CreateRequest {
    name: String,
}

async fn create(
    db: web::Data<MongoDatabase>,
    user: web::ReqData<Claims>,
    request: web::Json<CreateRequest>,
) -> actix_web::Result<impl Responder> {
    let collection = db.database.collection::<Chat>("chats");
    let message_collection = db.database.collection::<ChatMessage>("chat_messages");

    let mut chat = Chat {
        id: None,
        name: request.name.clone(),
        creator: user.user.id,
        user_ids: vec![user.user.id],
        first_message_ts: None,
        last_message_ts: None,
    };

    let inserted = collection
        .insert_one(&chat)
        .await
        .map_err(|err| error::ErrorInternalServerError(err))?;

    let chat_id = inserted
        .inserted_id
        .as_object_id()
        .context("missing inserted chat id")
        .map_err(|err| error::ErrorInternalServerError(err))?;

    chat.id = Some(chat_id);

    let ts = DateTime::now();

    let msg_future = message_collection
        .insert_one(ChatMessage {
            id: None,
            chat_id,
            creator: None,
            created_at: ts,
            deleted_at: None,
            content: String::from("Chat created"),
        })
        .into_future()
        .map_err(|err| error::ErrorInternalServerError(err));

    let chat_future = collection
        .update_one(
            doc! {
                "_id": chat_id
            },
            doc! {
                "$set": {
                    "first_message_ts": ts,
                    "last_message_ts": ts
                }
            },
        )
        .into_future()
        .map_err(|err| error::ErrorInternalServerError(err));

    try_join!(msg_future, chat_future)?;

    Ok(web::Json(chat))
}

#[derive(Serialize)]
struct JoinResponse {}

async fn join(
    db: web::Data<MongoDatabase>,
    user: web::ReqData<Claims>,
    id: web::Path<String>,
) -> actix_web::Result<impl Responder> {
    let collection = db.database.collection::<Chat>("chats");

    let chat_id =
        ObjectId::parse_str(id.into_inner()).map_err(|err| error::ErrorBadRequest(err))?;

    let chat = collection
        .find_one(doc! {
            "_id": &chat_id
        })
        .await
        .map_err(|err| error::ErrorInternalServerError(err))?
        .context("chat not found")
        .map_err(|err| error::ErrorNotFound(err))?;

    if chat.user_ids.contains(&user.user.id) {
        return Ok(web::Json(JoinResponse {}));
    }

    db.database
        .collection::<Chat>("chats")
        .update_one(
            doc! {
                "_id": &chat_id
            },
            doc! {
              "$push": { "user_ids": &user.user.id }
            },
        )
        .await
        .map_err(|err| error::ErrorInternalServerError(err))?;

    Ok(web::Json(JoinResponse {}))
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/", web::post().to(create))
        .route("/{id}", web::patch().to(join));
}
