use bson::{oid::ObjectId, DateTime};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Chat {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub creator: ObjectId,
    pub name: String,
    pub user_ids: Vec<ObjectId>,
    pub first_message_ts: Option<DateTime>,
    pub last_message_ts: Option<DateTime>,
}
