use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct CreateRequest {
    pub name: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct JoinResponse {}
