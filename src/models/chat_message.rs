use mongodb::bson::{oid::ObjectId, DateTime};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ChatMessage {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub chat_id: ObjectId,
    pub creator: Option<ObjectId>,
    pub created_at: DateTime,
    pub deleted_at: Option<DateTime>,
    pub content: String,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct ChatMessageSafe {
    #[serde(rename = "_id")]
    pub id: ObjectId,
    pub chat_id: ObjectId,
    pub creator: Option<ObjectId>,
    pub created_at: DateTime,
    pub deleted_at: Option<DateTime>,
    pub content: String,
}

impl From<ChatMessage> for ChatMessageSafe {
    fn from(value: ChatMessage) -> Self {
        ChatMessageSafe {
            id: value.id.expect("converting create payload into safe"),
            chat_id: value.chat_id,
            creator: value.creator,
            created_at: value.created_at,
            deleted_at: value.deleted_at,
            content: value.content,
        }
    }
}
