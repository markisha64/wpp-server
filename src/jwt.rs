use std::env;

use actix_web::web;
use jsonwebtoken::{encode, EncodingKey, Header};
use serde::Serialize;

pub struct JwtService {
    key: EncodingKey,
}

impl JwtService {
    pub fn init() -> anyhow::Result<web::Data<Self>> {
        let key = EncodingKey::from_secret(env::var("MONGODB_URL")?.as_ref());

        Ok(web::Data::new(JwtService { key }))
    }

    pub fn sign(&self, claims: &impl Serialize) -> Result<String, jsonwebtoken::errors::Error> {
        encode(&Header::default(), claims, &self.key)
    }
}
