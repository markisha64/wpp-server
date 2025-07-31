use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone)]
pub struct UploadFileResponse {
    pub path: String,
}
