use bson::oid::ObjectId;
use serde::{Deserialize, Serialize};

use crate::models::user::UserSafe;

#[derive(Serialize, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
}

#[derive(Serialize, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize, Deserialize)]
pub struct AuthResponse {
    pub user: UserSafe,
    pub token: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Claims {
    pub user_id: ObjectId,
    pub exp: usize,
}

#[derive(Serialize, Deserialize)]
pub struct UpdateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_image: Option<String>,
}
