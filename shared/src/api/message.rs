use bson::oid::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct CreateRequest {
    pub chat_id: ObjectId,
    pub content: String,
}
