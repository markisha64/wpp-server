use std::env;

use chrono::{Duration, Utc};

use actix_web::{
    error::{self},
    web, Responder,
};
use anyhow::Context;
use bcrypt::{hash, verify};
use mongodb::bson::{doc, oid::ObjectId};
use serde::Serialize;

use crate::{jwt::JwtSignService, mongodb::MongoDatabase, redis::RedisHandle};
use shared::{
    api::{
        user::{AuthResponse, Claims, LoginRequest, RegisterRequest, UpdateRequest},
        websocket::WebsocketServerMessage,
    },
    models::{self, user::UserSafe},
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
        .map_err(error::ErrorInternalServerError)?;

    let mut user = models::user::User {
        id: None,
        profile_image: String::new(),
        email: request.email.clone(),
        password_hash: hash.clone(),
        display_name: request.display_name.clone(),
    };

    let inserted = collection
        .insert_one(&user)
        .await
        .map_err(error::ErrorInternalServerError)?;

    user.id = inserted.inserted_id.as_object_id();

    let user: UserSafe = user.into();

    let claims = Claims {
        user_id: user.id,
        exp: (Utc::now() + Duration::days(1)).timestamp() as usize,
    };

    let token = jwt.sign(&claims).map_err(error::ErrorInternalServerError)?;

    Ok(web::Json(AuthResponse { token, user }))
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
        .map_err(error::ErrorInternalServerError)?
        .context("incorrect email/password")
        .map_err(error::ErrorNotFound)?;

    let password = request.password.clone();
    let password_hash = user.password_hash.clone();

    let correct = web::block(move || verify(password, &password_hash))
        .await?
        .map_err(error::ErrorInternalServerError)?;

    if !correct {
        return Err(error::ErrorNotFound("incorrect email/password"));
    }

    let user: models::user::UserSafe = user.into();

    let claims = Claims {
        user_id: user.id,
        exp: (Utc::now() + Duration::days(1)).timestamp() as usize,
    };

    let token = jwt.sign(&claims).map_err(error::ErrorInternalServerError)?;

    Ok(web::Json(AuthResponse { user, token }))
}

pub async fn update(
    db: web::Data<MongoDatabase>,
    user: &UserSafe,
    request: UpdateRequest,
    redis_handle: web::Data<RedisHandle>,
) -> anyhow::Result<UserSafe> {
    let collection = db.database.collection::<models::user::User>("users");

    let mut update = doc! {};

    if let Some(display_name) = &request.display_name {
        update.insert("display_name", display_name);
    }

    if let Some(profile_image) = &request.profile_image {
        update.insert("profile_image", profile_image);
    }

    let _ = collection
        .update_one(
            doc! {
                "_id": user.id
            },
            doc! {
                "$set": update
            },
        )
        .await?;

    let user = get_single(db, &user.id).await?;

    let user_ids = vec![user.id];
    let message = WebsocketServerMessage::ProfileUpdated(user.clone());

    actix_web::rt::spawn(async move {
        redis_handle.send_message_to_users(&user_ids, message).await;
    });

    Ok(user)
}

pub async fn get_single(
    db: web::Data<MongoDatabase>,
    user_id: &ObjectId,
) -> anyhow::Result<UserSafe> {
    let collection = db.database.collection::<models::user::User>("users");

    let user = collection
        .find_one(doc! {
            "_id": user_id
        })
        .await?
        .context("Missing user")?;

    Ok(user.into())
}

#[derive(Serialize)]
struct CFRequest {
    ttl: i32,
}

pub async fn get_turn_creds() -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let res = client
        .post(format!(
            "https://rtc.live.cloudflare.com/v1/turn/keys/{}/credentials/generate-ice-servers",
            env::var("CF_ID")?
        ))
        .header("Authorization", format!("Bearer {}", env::var("CF_TOKEN")?))
        .json(&CFRequest { ttl: 86400 })
        .send()
        .await?
        .text()
        .await?;

    Ok(res)
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("register", web::post().to(register))
        .route("login", web::post().to(login));
}
