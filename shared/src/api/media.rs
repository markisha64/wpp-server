use bson::oid::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct UploadFileRequest {
    pub name: String,
    pub bytes: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct UploadFileResponse {
    pub id: ObjectId,
}
