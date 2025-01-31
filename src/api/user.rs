use chrono::{Duration, Utc};

use actix_web::{error, web, Responder};
use anyhow::Context;
use bcrypt::{hash, verify};
use mongodb::bson::doc;
use serde::{Deserialize, Serialize};

use crate::{
    jwt::JwtService,
    models::{self, user::User},
    mongodb::MongoDatabase,
};

#[derive(Deserialize)]
struct RegisterRequest {
    email: String,
    password: String,
    display_name: String,
}

#[derive(Serialize)]
struct AuthResponse {
    token: String,
}

#[derive(Serialize)]
struct Claims {
    user: User,
    exp: usize,
}

async fn register(
    db: web::Data<MongoDatabase>,
    jwt: web::Data<JwtService>,
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

    let user = collection
        .insert_one(models::user::User {
            id: None,
            email: request.email.clone(),
            password_hash: hash.clone(),
            display_name: request.display_name.clone(),
        })
        .await
        .map_err(|err| error::ErrorInternalServerError(err))?;

    let user_data = collection
        .find_one(doc! {
            "_id": user.inserted_id
        })
        .await
        .map_err(|err| error::ErrorInternalServerError(err))?
        .context("failed to find created user")
        .map_err(|err| error::ErrorInternalServerError(err))?;

    let claims = Claims {
        user: user_data,
        exp: (Utc::now() + Duration::days(1)).timestamp() as usize,
    };

    let token = jwt
        .sign(&claims)
        .map_err(|err| error::ErrorInternalServerError(err))?;

    Ok(web::Json(AuthResponse { token }))
}

#[derive(Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

async fn login(
    db: web::Data<MongoDatabase>,
    jwt: web::Data<JwtService>,
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
        user,
        exp: (Utc::now() + Duration::days(1)).timestamp() as usize,
    };

    let token = jwt
        .sign(&claims)
        .map_err(|err| error::ErrorInternalServerError(err))?;

    Ok(web::Json(AuthResponse { token }))
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("register", web::post().to(register))
        .route("login", web::post().to(login));
}
