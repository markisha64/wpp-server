use mongodb::bson::oid::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug)]
pub struct User {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub email: String,
    pub password_hash: String,
    pub display_name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UserSafe {
    #[serde(rename = "_id")]
    pub id: ObjectId,
    pub email: String,
    pub display_name: String,
}

impl From<User> for UserSafe {
    fn from(value: User) -> Self {
        UserSafe {
            id: value.id.expect("converting create payload into safe"),
            email: value.email,
            display_name: value.display_name,
        }
    }
}
