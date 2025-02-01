use actix_web::{error, web, Responder};
use anyhow::Context;
use mongodb::bson::doc;
use serde::Deserialize;

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

    let inserted = collection
        .insert_one(Chat {
            id: None,
            name: request.name.clone(),
            creator: user.user.id,
            user_ids: vec![user.user.id],
        })
        .await
        .map_err(|err| error::ErrorInternalServerError(err))?;

    let chat = collection
        .find_one(doc! {
            "_id": inserted.inserted_id
        })
        .await
        .map_err(|err| error::ErrorInternalServerError(err))?
        .context("failed to find created chat")
        .map_err(|err| error::ErrorInternalServerError(err))?;

    Ok(web::Json(chat))
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/", web::post().to(create));
}
