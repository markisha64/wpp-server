use api::user::config;
use mongodb::MongoDatabase;

use actix_web::{web, App, HttpServer};

mod api;
mod models;
mod mongodb;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let mongo_database = MongoDatabase::init()
        .await
        .expect("Failed to initialize MongoDB client");

    HttpServer::new(move || {
        App::new()
            .app_data(mongo_database.to_owned())
            .service(web::scope("/user").configure(config))
    })
    .bind("127.0.0.1:3030")?
    .run()
    .await
}
