use api::user::config;
use dotenv::dotenv;
use jwt::JwtService;
use mongodb::MongoDatabase;

use actix_web::{web, App, HttpServer};

mod api;
mod jwt;
mod models;
mod mongodb;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();

    let mongo_database = MongoDatabase::init()
        .await
        .expect("Failed to initialize MongoDB client");

    let jwt_service = JwtService::init().expect("Failed to initialize JWT service");

    HttpServer::new(move || {
        App::new()
            .app_data(mongo_database.to_owned())
            .app_data(jwt_service.to_owned())
            .service(web::scope("/user").configure(config))
    })
    .bind("127.0.0.1:3030")?
    .run()
    .await
}
