use actix_web::web;
use mongodb::{options::ClientOptions, Client, Database};

#[derive(Clone)]
pub struct MongoDatabase {
    pub database: Database,
}

impl MongoDatabase {
    pub async fn init() -> anyhow::Result<web::Data<Self>> {
        let client_options = ClientOptions::parse("mongodb://localhost:27017").await?;
        let client = Client::with_options(client_options)?;

        Ok(web::Data::new(MongoDatabase {
            database: client.database("wpp"),
        }))
    }
}
