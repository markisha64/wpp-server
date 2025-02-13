use std::future::IntoFuture;

use actix_web::web;
use anyhow::{anyhow, Context};
use futures_util::{try_join, TryStreamExt};
use mongodb::{
    bson::{doc, DateTime},
    options::FindOptions,
};

use crate::{mongodb::MongoDatabase, redis::RedisHandle};
use shared::{
    api::{
        message::{CreateRequest, GetRequest},
        user::Claims,
        websocket::WebsocketServerMessage,
    },
    models::{
        chat::Chat,
        chat_message::{ChatMessage, ChatMessageSafe},
    },
};

pub async fn create(
    db: web::Data<MongoDatabase>,
    user: &web::ReqData<Claims>,
    redis_handle: web::Data<RedisHandle>,
    request: CreateRequest,
) -> anyhow::Result<ChatMessageSafe> {
    let chat_collection = db.database.collection::<Chat>("chats");
    let message_collection = db.database.collection::<ChatMessage>("chat_messages");

    let chat = chat_collection
        .find_one(doc! {
            "_id": &request.chat_id
        })
        .await?
        .context("chat not found")?;

    if !chat.user_ids.contains(&user.user.id) {
        return Err(anyhow!("chat not found"));
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

    let message_future = message_collection.insert_one(&message).into_future();

    let chat_future = chat_collection
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
        .into_future();

    let (message_id, _) = try_join!(message_future, chat_future)?;

    message.id = message_id.inserted_id.as_object_id();

    let notif_payload = WebsocketServerMessage::NewMessage(message.clone().into());

    actix_web::rt::spawn(async move {
        redis_handle
            .send_message_to_users(&chat.user_ids, notif_payload)
            .await;
    });

    Ok(message.into())
}

pub async fn get_messages(
    db: web::Data<MongoDatabase>,
    user: &web::ReqData<Claims>,
    request: GetRequest,
) -> anyhow::Result<Vec<ChatMessageSafe>> {
    let chat_collection = db.database.collection::<Chat>("chats");
    let collection = db.database.collection::<ChatMessageSafe>("chat_messages");

    let _ = chat_collection
        .find_one(doc! {
            "_id": &request.chat_id,
            "user_ids": &user.user.id
        })
        .await?
        .context("chat not found")?;

    let messages: Vec<_> = collection
        .find(doc! {
            "chat_id": &request.chat_id,
            "created_at": {
                "$lt": &request.last_message_ts
            }
        })
        .limit(10)
        .sort(doc! {
            "created_at": -1
        })
        .await?
        .try_collect()
        .await?;

    Ok(Vec::from_iter(messages.into_iter().rev()))
}
