use bson::{oid::ObjectId, DateTime};
use mediasoup::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    api::chat::JoinResponse,
    models::{chat::ChatSafe, chat_message::ChatMessageSafe, chat_user::ChatUserPopulated},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum WebsocketServerMessage {
    NewMessage(ChatMessageSafe),
    UserJoined {
        chat_id: ObjectId,
        user: ChatUserPopulated,
    },
    RequestResponse {
        id: Uuid,
        data: Result<WebsocketServerResData, String>,
    },
    SetChatRead {
        chat_id: ObjectId,
        last_message_ts: DateTime,
    },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum WebsocketServerResData {
    // chat routes
    CreateChat(ChatSafe),
    JoinChat(JoinResponse),
    GetChats(Vec<ChatSafe>),
    SetChatRead(DateTime),

    // message routes
    NewMessage(ChatMessageSafe),
    GetMessages(Vec<ChatMessageSafe>),

    // mediasoup
    SetRoom,
    RtpInit,
    ConnectProducerTransport,
    Produce(ProducerId),
    ConnectConsumerTransport,
    Consume {
        id: ConsumerId,
        producer_id: ProducerId,
        kind: MediaKind,
        rtp_parameters: RtpParameters,
    },
    ConsumerResume,
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
    SetChatRead(ObjectId),

    // message routes
    NewMessage(crate::api::message::CreateRequest),
    GetMessages(crate::api::message::GetRequest),

    // mediasoup
    MS(MediaSoup),
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum MediaSoup {
    RtpInit(RtpCapabilities),
    ConnectProducerTransport(DtlsParameters),
    Produce((MediaKind, RtpParameters)),
    ConnectConsumerTransport(DtlsParameters),
    Consume(ProducerId),
    ConsumerResume(ConsumerId),
    SetRoom(ObjectId),
}
