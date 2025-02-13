use actix_web::web;
use anyhow::Context;
use mongodb::bson::{doc, oid::ObjectId, DateTime};
use std::future::IntoFuture;

use futures_util::{try_join, TryStreamExt};

use shared::{
    api::{
        chat::{CreateRequest, JoinResponse},
        user::Claims,
    },
    models::{
        chat::{Chat, ChatSafe},
        chat_message::ChatMessage,
    },
};

use crate::mongodb::MongoDatabase;

pub async fn create(
    db: web::Data<MongoDatabase>,
    user: &web::ReqData<Claims>,
    request: CreateRequest,
) -> anyhow::Result<Chat> {
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

    let inserted = collection.insert_one(&chat).await?;

    let chat_id = inserted
        .inserted_id
        .as_object_id()
        .context("missing inserted chat id")?;

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
        .into_future();

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
        .into_future();

    try_join!(msg_future, chat_future)?;

    Ok(chat)
}

pub async fn join(
    db: web::Data<MongoDatabase>,
    user: &web::ReqData<Claims>,
    chat_id: ObjectId,
) -> anyhow::Result<JoinResponse> {
    let collection = db.database.collection::<Chat>("chats");

    let chat = collection
        .find_one(doc! {
            "_id": &chat_id
        })
        .await?
        .context("chat not found")?;

    if chat.user_ids.contains(&user.user.id) {
        return Ok(JoinResponse {});
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
        .await?;

    Ok(JoinResponse {})
}

pub async fn get_chats(
    db: web::Data<MongoDatabase>,
    user: &web::ReqData<Claims>,
) -> anyhow::Result<Vec<ChatSafe>> {
    let collection = db.database.collection::<ChatSafe>("chats");

    let chats = collection
        .find(doc! {
            "user_ids": &user.user.id
        })
        .sort(doc! {
            "last_message_ts": -1
        })
        .await?
        .try_collect::<Vec<_>>()
        .await?;

    Ok(chats)
}
