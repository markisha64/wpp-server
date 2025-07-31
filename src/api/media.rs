use std::path::Path;

use actix_files as fs;
use actix_multipart::form::{tempfile::TempFile, MultipartForm};
use actix_web::{error::ErrorInternalServerError, web, Responder};
use anyhow::Context;
use shared::api::{media::UploadFileResponse, user::Claims};
use uuid::Uuid;

use crate::jwt::JwtAuth;

#[derive(MultipartForm)]
pub struct UploadFileRequest {
    file: TempFile,
}

pub async fn upload_file(
    _: web::ReqData<Claims>,
    MultipartForm(req_data): MultipartForm<UploadFileRequest>,
) -> actix_web::Result<impl Responder> {
    let uuid = Uuid::new_v4().to_string();
    let file_name = req_data
        .file
        .file_name
        .context("missing file name")
        .map_err(|err| ErrorInternalServerError(err))?;

    let ext = Path::new(&file_name)
        .extension()
        .context("missing ext")
        .map_err(|err| ErrorInternalServerError(err))?
        .to_str()
        .context("failed to convert to str")
        .map_err(|err| ErrorInternalServerError(err))?;

    let path = format!("./media/{}.{}", uuid, ext);

    req_data
        .file
        .file
        .persist(path)
        .map_err(|x| ErrorInternalServerError(x))?;

    Ok(web::Json(UploadFileResponse {
        path: format!("/media/{}.{}", uuid, ext),
    }))
}

pub fn config(cfg: &mut web::ServiceConfig, jwt_auth: JwtAuth<Claims>) {
    cfg.service(
        web::resource("/upload")
            .route(web::post().to(upload_file))
            .wrap(jwt_auth),
    )
    .service(fs::Files::new("/", "./media").show_files_listing());
}

pub fn config_wrapper(jwt_auth: &JwtAuth<Claims>) -> impl FnOnce(&mut web::ServiceConfig) {
    let jwt_auth = jwt_auth.to_owned();

    move |cfg: &mut web::ServiceConfig| config(cfg, jwt_auth)
}
