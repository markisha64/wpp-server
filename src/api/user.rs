use actix_web::{web, HttpResponse, Responder};
use bcrypt::hash;
use mongodb::bson::doc;
use serde::Deserialize;

use crate::{models, mongodb::MongoDatabase};

#[derive(Deserialize)]
struct RegisterRequest {
    email: String,
    password: String,
    display_name: String,
}

async fn register(
    db: web::Data<MongoDatabase>,
    request: web::Json<RegisterRequest>,
) -> impl Responder {
    let collection = db.database.collection::<models::user::User>("users");

    let exists = collection.find_one(doc! {
        "email": &request.email
    });

    if let Ok(res) = exists.await {
        if res.is_some() {
            return HttpResponse::BadRequest().body("email already exists");
        }
    }

    let password = request.password.clone();
    let hash = match web::block(|| hash(password, 10)).await {
        Ok(s) => match s {
            Ok(s) => s,
            Err(e) => {
                return HttpResponse::InternalServerError().body(format!("Error: {}", e));
            }
        },
        Err(e) => {
            return HttpResponse::InternalServerError().body(format!("Error: {}", e));
        }
    };

    let user = collection.insert_one(models::user::User {
        id: None,
        email: request.email.clone(),
        password_hash: hash.clone(),
        display_name: request.display_name.clone(),
    });

    match user.await {
        Ok(new_user) => HttpResponse::Created().json(new_user),
        Err(e) => HttpResponse::InternalServerError().body(format!("Error: {}", e)),
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("register", web::post().to(register));
}
