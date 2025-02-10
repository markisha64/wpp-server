use api::{user::Claims, websocket::WebsocketServer};
use dotenv::dotenv;
use jwt::{JwtAuth, JwtSignService};
use mongodb::MongoDatabase;
use redis::RedisHandler;
use std::{env, io};

use actix_web::{web, App, HttpServer};
use tokio::{task::spawn, try_join};

mod api;
mod jwt;
mod models;
mod mongodb;
mod redis;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();

    let mongo_database = MongoDatabase::init()
        .await
        .expect("Failed to initialize MongoDB client");

    let jwt_service = JwtSignService::init().expect("Failed to initialize JWT service");

    let jwt_auth = JwtAuth::<Claims>::init().expect("failed to auth init");

    let (ws_server, server_tx) = WebsocketServer::new(mongo_database.to_owned());

    let (redis_handler, redis_handle) =
        RedisHandler::new(server_tx.clone()).expect("Failed to create redis handler");

    let ws_fut = spawn(ws_server.run());

    let redis_fut = spawn(redis_handler.run());

    let http_fut = HttpServer::new(move || {
        App::new()
            .app_data(mongo_database.to_owned())
            .app_data(jwt_service.to_owned())
            .app_data(web::Data::new(server_tx.clone()))
            .app_data(web::Data::new(redis_handle.clone()))
            .service(web::scope("/user").configure(api::user::config))
            .service(
                web::scope("/ws")
                    .wrap(jwt_auth.to_owned())
                    .configure(api::websocket::config),
            )
    })
    .bind(format!(
        "127.0.0.1:{}",
        env::var("PORT").unwrap_or("3030".to_string())
    ))?
    .run();

    try_join!(http_fut, async move { ws_fut.await.unwrap() }, async move {
        redis_fut
            .await
            .unwrap()
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
    })?;

    Ok(())
}
