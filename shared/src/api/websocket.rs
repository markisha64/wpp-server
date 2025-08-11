use bson::{oid::ObjectId, DateTime};
use mediasoup_types::{
    data_structures::{DtlsParameters, IceCandidate, IceParameters},
    rtp_parameters::{MediaKind, RtpCapabilities, RtpCapabilitiesFinalized, RtpParameters},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    api::chat::JoinResponse,
    models::{
        chat::ChatSafe, chat_message::ChatMessageSafe, chat_user::ChatUserPopulated, user::UserSafe,
    },
};

#[derive(Serialize, Deserialize, Clone)]
pub struct TransportOptions {
    /// TransportId
    pub id: String,
    pub dtls_parameters: DtlsParameters,
    pub ice_candidates: Vec<IceCandidate>,
    pub ice_parameters: IceParameters,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum WebsocketServerMessage {
    ProfileUpdated(UserSafe),
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
    ProducerAdded {
        participant_id: String,
        /// ProducerId
        producer_id: String,
    },
    ProducerRemove {
        participant_id: String,
        /// ProducerId
        producer_id: String,
    },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum WebsocketServerResData {
    /// other
    ProfileUpdate(UserSafe),
    GetSelf(UserSafe),

    /// chat routes
    CreateChat(ChatSafe),
    JoinChat(JoinResponse),
    GetChats(Vec<ChatSafe>),
    SetChatRead(DateTime),

    /// message routes
    NewMessage(ChatMessageSafe),
    GetMessages(Vec<ChatMessageSafe>),

    /// mediasoup
    SetRoom {
        room_id: String,
        consumer_transport_options: TransportOptions,
        producer_transport_options: TransportOptions,
        router_rtp_capabilities: RtpCapabilitiesFinalized,
        /// ProducerId
        producers: Vec<(String, String)>,
    },
    ConnectProducerTransport,
    /// ProducerId
    Produce(String),
    ConnectConsumerTransport,
    Consume {
        /// ConsumerId
        id: String,
        /// ProducerId
        producer_id: String,
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
    /// other
    GetSelf,
    ProfileUpdate(crate::api::user::UpdateRequest),

    /// chat routes
    CreateChat(crate::api::chat::CreateRequest),
    JoinChat(ObjectId),
    GetChats,
    SetChatRead(ObjectId),

    /// message routes
    NewMessage(crate::api::message::CreateRequest),
    GetMessages(crate::api::message::GetRequest),

    /// mediasoup
    MS(MediaSoup),
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum MediaSoup {
    ConnectProducerTransport(DtlsParameters),
    Produce((MediaKind, RtpParameters)),
    ConnectConsumerTransport(DtlsParameters),
    /// ProducerId
    Consume(String),
    /// ConsumerId
    ConsumerResume(String),
    SetRoom((ObjectId, RtpCapabilities)),
}
