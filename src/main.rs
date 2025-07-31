use actix_cors::Cors;
use api::websocket::WebsocketServer;
use dotenv::dotenv;
use jwt::{JwtAuth, JwtSignService};
use mediasoup::prelude::WorkerManager;
use mongodb::MongoDatabase;
use redis::RedisHandler;
use shared::api::user::Claims;
use std::{env, io};

use actix_web::{http, web, App, HttpServer};
use tokio::{task::spawn, try_join};

mod api;
mod jwt;
mod mongodb;
mod redis;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();

    let worker_manager = web::Data::new(WorkerManager::new());

    let mongo_database = MongoDatabase::init()
        .await
        .expect("Failed to initialize MongoDB client");

    let jwt_service = JwtSignService::init().expect("Failed to initialize JWT service");

    let jwt_auth = JwtAuth::<Claims>::init().expect("failed to auth init");

    let (ws_server, server_tx) =
        WebsocketServer::new(mongo_database.to_owned(), worker_manager.to_owned());

    let ws_handle = web::Data::new(server_tx);

    let (redis_handler, redis_handle) =
        RedisHandler::new(ws_handle.clone()).expect("Failed to create redis handler");

    let ws_fut = spawn(ws_server.run());

    let redis_fut = spawn(redis_handler.run());

    let addr = format!(
        "{}:{}",
        env::var("HOST").unwrap_or("0.0.0.0".to_string()),
        env::var("PORT").unwrap_or("3030".to_string())
    );

    let http_fut = HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .send_wildcard()
            .allowed_methods(vec!["POST", "GET"])
            .allowed_headers(vec![
                http::header::AUTHORIZATION,
                http::header::ACCEPT,
                http::header::CONTENT_TYPE,
            ])
            .max_age(3600);

        App::new()
            .wrap(cors)
            .app_data(worker_manager.to_owned())
            .app_data(mongo_database.to_owned())
            .app_data(jwt_service.to_owned())
            .app_data(ws_handle.clone())
            .app_data(web::Data::new(redis_handle.clone()))
            .service(web::scope("/user").configure(api::user::config))
            .service(web::scope("/media").configure(api::media::config_wrapper(&jwt_auth)))
            .service(
                web::scope("/user_auth")
                    .wrap(jwt_auth.to_owned())
                    .configure(api::user::config_auth),
            )
            .service(
                web::scope("/ws")
                    .wrap(jwt_auth.to_owned())
                    .configure(api::websocket::config),
            )
    })
    .bind(&addr)?
    .run();

    println!("binding on {}", addr);

    try_join!(http_fut, async move { ws_fut.await.unwrap() }, async move {
        redis_fut
            .await
            .unwrap()
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
    })?;

    Ok(())
}
