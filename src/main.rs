use std::sync::{Arc, Mutex};

use ::mongodb::Client;
use actix_web::{get, web, App, HttpResponse, HttpServer, Responder};
use mongodb::create_mongo_client;

mod mongodb;

#[get("/test")]
async fn test() -> impl Responder {
    HttpResponse::Ok().body("Hello world!")
}

pub struct AppState {
    mongodb_client: Arc<Mutex<Client>>,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let mongodb_client = create_mongo_client()
        .await
        .expect("Failed to initialize MongoDB client");
    let app_state = web::Data::new(AppState {
        mongodb_client: Arc::new(Mutex::new(mongodb_client)),
    });

    HttpServer::new(move || App::new().service(test).app_data(app_state.clone()))
        .bind("127.0.0.1:3030")?
        .run()
        .await
}
