use bson::{oid::ObjectId, DateTime};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ChatUser {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub chat_id: ObjectId,
    pub user_id: ObjectId,
    pub last_message_seen_ts: DateTime,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ChatUserPopulated {
    #[serde(rename = "_id")]
    pub id: ObjectId,
    pub last_message_seen_ts: DateTime,
    pub display_name: String,
    pub profile_image: String,
}
