use actix_web::web;
use anyhow::Context;
use mongodb::bson::{self, doc, oid::ObjectId, DateTime};
use std::future::IntoFuture;

use futures_util::{try_join, StreamExt, TryStreamExt};

use shared::{
    api::{
        chat::{CreateRequest, JoinResponse},
        user::Claims,
        websocket::WebsocketServerMessage,
    },
    models::{
        chat::{Chat, ChatSafe},
        chat_message::ChatMessage,
        chat_user::{ChatUser, ChatUserPopulated},
    },
};

use crate::{mongodb::MongoDatabase, redis::RedisHandle};

pub async fn create(
    db: web::Data<MongoDatabase>,
    user: &web::ReqData<Claims>,
    request: CreateRequest,
) -> anyhow::Result<ChatSafe> {
    let collection = db.database.collection::<Chat>("chats");
    let message_collection = db.database.collection::<ChatMessage>("chat_messages");
    let chat_user_collection = db.database.collection::<ChatUser>("chat_users");

    let mut chat = Chat {
        id: None,
        name: request.name.clone(),
        creator: user.user.id,
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

    let mut msg = ChatMessage {
        id: None,
        chat_id,
        creator: None,
        created_at: ts,
        deleted_at: None,
        content: String::from("Chat created"),
    };
    let msg_future = message_collection.insert_one(&msg).into_future();

    let chat_user_future = chat_user_collection
        .insert_one(ChatUser {
            id: None,
            chat_id,
            user_id: user.user.id,
            last_message_seen_ts: ts,
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

    let (msg_insert, _, _) = try_join!(msg_future, chat_future, chat_user_future)?;

    msg.id = msg_insert.inserted_id.as_object_id();

    chat.first_message_ts = Some(ts);
    chat.last_message_ts = Some(ts);

    let mut safe: ChatSafe = chat.into();

    safe.messages.push(msg.into());
    safe.users.push(ChatUserPopulated {
        id: user.user.id,
        last_message_seen_ts: ts,
        display_name: user.user.display_name.clone(),
    });

    Ok(safe)
}

pub async fn get_single(
    db: web::Data<MongoDatabase>,
    chat_id: ObjectId,
) -> anyhow::Result<ChatSafe> {
    let collection = db.database.collection::<ChatSafe>("chats");

    let chat = collection
        .aggregate(vec![
            doc! {
                "$match": {
                    "_id": chat_id
                },
            },
            doc! {
                "$lookup": {
                    "from": "chat_users",
                    "localField": "_id",
                    "foreignField": "chat_id",
                    "as": "users",
                    "pipeline": vec![
                        doc! {
                            "$lookup": {
                                "from": "users",
                                "localField": "user_id",
                                "foreignField": "_id",
                                "as": "user"
                            },
                        },
                        doc! {
                            "$unwind": "$user"
                        },
                        doc! {
                            "$project": {
                                "_id": "$user._id",
                                "last_message_seen_ts": 1,
                                "display_name": "$user.display_name"
                            }
                        }
                    ]
                }
            },
        ])
        .await?
        .next()
        .await
        .context("chat not found")?
        .map(|x| bson::from_bson::<ChatSafe>(bson::Bson::Document(x)))??;

    Ok(chat)
}

pub async fn join(
    db: web::Data<MongoDatabase>,
    user: &web::ReqData<Claims>,
    redis_handle: web::Data<RedisHandle>,
    chat_id: ObjectId,
) -> anyhow::Result<JoinResponse> {
    let chat = get_single(db.clone(), chat_id).await?;

    if chat.users.iter().find(|x| x.id == user.user.id).is_some() {
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

    let notif_payload = WebsocketServerMessage::UserJoined {
        chat_id: chat.id,
        user: user.user.clone(),
    };

    let user_ids = chat.users.iter().map(|x| x.id).collect();

    actix_web::rt::spawn(async move {
        redis_handle
            .send_message_to_users(&user_ids, notif_payload)
            .await;
    });

    Ok(JoinResponse {})
}

pub async fn get_chats(
    db: web::Data<MongoDatabase>,
    user: &web::ReqData<Claims>,
) -> anyhow::Result<Vec<ChatSafe>> {
    let collection = db.database.collection::<ChatSafe>("chats");

    let chats = collection
        .aggregate(vec![
            doc! {
                "$match": {
                    "user_ids": &user.user.id
                }
            },
            doc! {
                "$lookup": {
                    "from": "chat_users",
                    "localField": "_id",
                    "foreignField": "chat_id",
                    "as": "users",
                    "pipeline": vec![
                        doc! {
                            "$match": {
                                "user_id": user.user.id
                            }
                        },
                        doc! {
                            "$lookup": {
                                "from": "users",
                                "localField": "user_id",
                                "foreignField": "_id",
                                "as": "user"
                            },
                        },
                        doc! {
                            "$unwind": "$user"
                        },
                        doc! {
                            "$project": {
                                "_id": "$user._id",
                                "last_message_seen_ts": 1,
                                "display_name": "$user.display_name"
                            }
                        }
                    ]
                }
            },
            doc! {
                "$sort": {
                    "last_message_ts": -1
                }
            },
        ])
        .await?
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .map(|x| bson::from_bson::<ChatSafe>(bson::Bson::Document(x)))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(chats)
}
