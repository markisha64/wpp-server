use mongodb::{options::ClientOptions, Client};

pub async fn create_mongo_client() -> anyhow::Result<Client> {
    let client_options = ClientOptions::parse("mongodb://localhost:27017").await?;
    let client = Client::with_options(client_options)?;
    Ok(client)
}
