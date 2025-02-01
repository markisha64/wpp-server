use api::user::{config, Claims};
use dotenv::dotenv;
use jwt::{JwtAuth, JwtSignService};
use mongodb::MongoDatabase;

use actix_web::{web, App, HttpServer};

mod api;
mod jwt;
mod models;
mod mongodb;

async fn jwt_r(claims: web::ReqData<Claims>) -> &'static str {
    dbg!(claims);
    "testing"
}

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
            .service(web::scope("/user").configure(config))
            .service(
                web::scope("/test")
                    .wrap(jwt_auth.to_owned())
                    .route("/jwt", web::get().to(jwt_r)),
            )
    })
    .bind("127.0.0.1:3030")?
    .run()
    .await
}
