use bson::{oid::ObjectId, DateTime};
use serde::{Deserialize, Serialize};

use super::{chat_message::ChatMessageSafe, chat_user::ChatUserPopulated};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Chat {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub creator: ObjectId,
    pub name: String,
    pub first_message_ts: Option<DateTime>,
    pub last_message_ts: Option<DateTime>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ChatSafe {
    #[serde(rename = "_id")]
    pub id: ObjectId,
    pub creator: ObjectId,
    pub name: String,
    pub first_message_ts: DateTime,
    pub last_message_ts: DateTime,
    #[serde(default = "Vec::new")]
    pub messages: Vec<ChatMessageSafe>,
    #[serde(default = "Vec::new")]
    pub users: Vec<ChatUserPopulated>,
}

impl From<Chat> for ChatSafe {
    fn from(value: Chat) -> Self {
        ChatSafe {
            id: value.id.expect("converting create payloud into safe"),
            creator: value.creator,
            name: value.name,
            first_message_ts: value
                .first_message_ts
                .expect("converting create payloud into safe"),
            last_message_ts: value
                .last_message_ts
                .expect("converting create payloud into safe"),
            messages: Vec::new(),
            users: Vec::new(),
        }
    }
}
