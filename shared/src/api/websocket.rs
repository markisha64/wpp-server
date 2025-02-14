use bson::oid::ObjectId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    api::chat::JoinResponse,
    models::{
        chat::{Chat, ChatSafe},
        chat_message::ChatMessageSafe,
    },
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum WebsocketServerMessage {
    NewMessage(ChatMessageSafe),
    RequestResponse {
        id: Uuid,
        data: Result<WebsocketServerResData, String>,
    },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum WebsocketServerResData {
    // chat routes
    CreateChat(Chat),
    JoinChat(JoinResponse),
    GetChats(Vec<ChatSafe>),

    // message routes
    NewMessage(ChatMessageSafe),
    GetMessages(Vec<ChatMessageSafe>),
}

#[derive(Serialize, Deserialize)]
pub struct WebsocketClientMessage {
    pub id: Uuid,
    pub data: WebsocketClientMessageData,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum WebsocketClientMessageData {
    // chat routes
    CreateChat(crate::api::chat::CreateRequest),
    JoinChat(ObjectId),
    GetChats,

    // message routes
    NewMessage(crate::api::message::CreateRequest),
    GetMessages(crate::api::message::GetRequest),
}
