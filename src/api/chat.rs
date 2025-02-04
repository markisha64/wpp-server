use actix_web::{error, web, Responder};
use anyhow::Context;
use mongodb::bson::{doc, oid::ObjectId};
use serde::{Deserialize, Serialize};

use crate::{models::chat::Chat, mongodb::MongoDatabase};

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

    chat.id = inserted.inserted_id.as_object_id();

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
