use chrono::{Duration, Utc};

use actix_web::{error, web, Responder};
use anyhow::Context;
use bcrypt::{hash, verify};
use mongodb::bson::doc;

use crate::{jwt::JwtSignService, mongodb::MongoDatabase};
use shared::{
    api::user::{AuthResponse, Claims, LoginRequest, RegisterRequest, UpdateRequest},
    models::{self},
};

async fn register(
    db: web::Data<MongoDatabase>,
    jwt: web::Data<JwtSignService>,
    request: web::Json<RegisterRequest>,
) -> actix_web::Result<impl Responder> {
    let collection = db.database.collection::<models::user::User>("users");

    let exists = collection.find_one(doc! {
        "email": &request.email
    });

    if let Ok(res) = exists.await {
        if res.is_some() {
            return Err(error::ErrorBadRequest("email already exists"));
        }
    }

    let password = request.password.clone();
    let hash = web::block(|| hash(password, 10))
        .await?
        .map_err(|err| error::ErrorInternalServerError(err))?;

    let mut user = models::user::User {
        id: None,
        email: request.email.clone(),
        password_hash: hash.clone(),
        display_name: request.display_name.clone(),
    };

    let inserted = collection
        .insert_one(&user)
        .await
        .map_err(|err| error::ErrorInternalServerError(err))?;

    user.id = inserted.inserted_id.as_object_id();

    let claims = Claims {
        user: user.into(),
        exp: (Utc::now() + Duration::days(1)).timestamp() as usize,
    };

    let token = jwt
        .sign(&claims)
        .map_err(|err| error::ErrorInternalServerError(err))?;

    Ok(web::Json(AuthResponse { token }))
}

async fn login(
    db: web::Data<MongoDatabase>,
    jwt: web::Data<JwtSignService>,
    request: web::Json<LoginRequest>,
) -> actix_web::Result<impl Responder> {
    let collection = db.database.collection::<models::user::User>("users");

    let user = collection
        .find_one(doc! {
            "email": &request.email
        })
        .await
        .map_err(|err| error::ErrorInternalServerError(err))?
        .context("incorrect email/password")
        .map_err(|err| error::ErrorNotFound(err))?;

    let password = request.password.clone();
    let password_hash = user.password_hash.clone();

    let correct = web::block(move || verify(password, &password_hash))
        .await?
        .map_err(|err| error::ErrorInternalServerError(err))?;

    if !correct {
        return Err(error::ErrorNotFound("incorrect email/password"));
    }

    let claims = Claims {
        user: user.into(),
        exp: (Utc::now() + Duration::days(1)).timestamp() as usize,
    };

    let token = jwt
        .sign(&claims)
        .map_err(|err| error::ErrorInternalServerError(err))?;

    Ok(web::Json(AuthResponse { token }))
}

async fn update(
    db: web::Data<MongoDatabase>,
    user: web::ReqData<Claims>,
    request: web::Json<UpdateRequest>,
) -> actix_web::Result<impl Responder> {
    let collection = db.database.collection::<models::user::User>("users");

    let _ = collection
        .update_one(
            doc! {
                "_id": user.user.id
            },
            doc! {
                "display_name": &request.display_name
            },
        )
        .await
        .map_err(|err| error::ErrorInternalServerError(err))?;

    Ok(web::Json(()))
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("register", web::post().to(register))
        .route("login", web::post().to(login));
}

pub fn config_auth(cfg: &mut web::ServiceConfig) {
    cfg.route("update", web::patch().to(update));
}
