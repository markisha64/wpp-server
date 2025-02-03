use api::user::Claims;
use dotenv::dotenv;
use jwt::{JwtAuth, JwtSignService};
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

    let jwt_service = JwtSignService::init().expect("Failed to initialize JWT service");

    let jwt_auth = JwtAuth::<Claims>::init().expect("failed to auth init");

    HttpServer::new(move || {
        App::new()
            .app_data(mongo_database.to_owned())
            .app_data(jwt_service.to_owned())
            .service(web::scope("/user").configure(api::user::config))
            .service(
                web::scope("/chat")
                    .wrap(jwt_auth.to_owned())
                    .configure(api::chat::config),
            )
            .service(
                web::scope("/message")
                    .wrap(jwt_auth.to_owned())
                    .configure(api::message::config),
            )
    })
    .bind("127.0.0.1:3030")?
    .run()
    .await
}
