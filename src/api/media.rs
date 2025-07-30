use actix_web::web;
use mongodb::bson::oid::ObjectId;
use shared::api::media::{UploadFileRequest, UploadFileResponse};

use crate::mongodb::MongoDatabase;

pub async fn upload_file(
    db: web::Data<MongoDatabase>,
    req_data: UploadFileRequest,
) -> anyhow::Result<UploadFileResponse> {
    Ok(UploadFileResponse {
        id: ObjectId::new(),
    })
}
