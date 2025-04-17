use std::future::IntoFuture;

use actix_web::web;
use anyhow::{anyhow, Context};
use futures_util::{try_join, TryStreamExt};
use mongodb::bson::{doc, DateTime};

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

use super::chat::get_single;

pub async fn create(
    db: web::Data<MongoDatabase>,
    user: &web::ReqData<Claims>,
    redis_handle: web::Data<RedisHandle>,
    request: CreateRequest,
) -> anyhow::Result<ChatMessageSafe> {
    let chat_collection = db.database.collection::<Chat>("chats");
    let message_collection = db.database.collection::<ChatMessage>("chat_messages");

    let chat = get_single(db.clone(), request.chat_id).await?;

    if chat.users.iter().find(|x| x.id == user.user.id).is_none() {
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

    let user_ids = chat.users.iter().map(|x| x.id).collect();

    actix_web::rt::spawn(async move {
        redis_handle
            .send_message_to_users(&user_ids, notif_payload)
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

    let mut filter = doc! {
        "chat_id": &request.chat_id,
    };

    if let Some(ts) = request.last_message_ts {
        filter.insert(
            "created_at",
            doc! {
                "$lt": ts
            },
        );
    }

    let messages: Vec<_> = collection
        .find(filter)
        .limit(10)
        .sort(doc! {
            "created_at": -1
        })
        .await?
        .try_collect()
        .await?;

    Ok(Vec::from_iter(messages.into_iter().rev()))
}
