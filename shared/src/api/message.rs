use bson::{oid::ObjectId, DateTime};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct CreateRequest {
    pub chat_id: ObjectId,
    pub content: String,
}

#[derive(Serialize, Deserialize)]
pub struct GetRequest {
    pub chat_id: ObjectId,
    pub last_message_ts: DateTime,
}
