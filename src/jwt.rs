use std::marker::PhantomData;
use std::sync::Arc;
use std::{env, rc::Rc};

use futures_util::future::LocalBoxFuture;
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::future::{ready, Ready};

use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    error::ErrorUnauthorized,
    http::header,
    web, Error, HttpMessage,
};
use jsonwebtoken::{encode, EncodingKey, Header};

pub struct JwtSignService {
    key: EncodingKey,
}

impl JwtSignService {
    pub fn init() -> anyhow::Result<web::Data<Self>> {
        let key = EncodingKey::from_secret(env::var("SECRET_KEY")?.as_ref());

        Ok(web::Data::new(JwtSignService { key }))
    }

    pub fn sign(&self, claims: &impl Serialize) -> Result<String, jsonwebtoken::errors::Error> {
        encode(&Header::default(), claims, &self.key)
    }
}

#[derive(Clone)]
pub struct JwtAuth<T: DeserializeOwned> {
    key: Arc<DecodingKey>,
    phantom: PhantomData<T>,
}

impl<T: DeserializeOwned> JwtAuth<T> {
    pub fn init() -> anyhow::Result<Self> {
        let key = DecodingKey::from_secret(env::var("SECRET_KEY")?.as_ref());

        Ok(JwtAuth {
            key: Arc::new(key),
            phantom: PhantomData,
        })
    }
}

impl<S, B, T> Transform<S, ServiceRequest> for JwtAuth<T>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
    T: DeserializeOwned + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = JwtAuthMiddleware<S, T>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(JwtAuthMiddleware {
            service,
            key: self.key.clone(),
            phantom: self.phantom,
        }))
    }
}

#[derive(Clone)]
pub struct JwtAuthMiddleware<S, T> {
    key: Arc<DecodingKey>,
    service: S,
    phantom: PhantomData<T>,
}

impl<S, B, T> Service<ServiceRequest> for JwtAuthMiddleware<S, T>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
    T: DeserializeOwned + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        println!("Hi from start. You requested: {}", req.path());

        let token = match extract_token(&req) {
            Some(token) => token,
            None => {
                return Box::pin(ready(Err(ErrorUnauthorized("No token found"))));
            }
        };

        let token_data = match decode::<T>(&token, &self.key, &Validation::default()) {
            Ok(token_data) => token_data,
            Err(_) => {
                return Box::pin(ready(Err(ErrorUnauthorized("Invalid token"))));
            }
        };

        req.extensions_mut().insert(token_data.claims);

        let fut = self.service.call(req);

        Box::pin(async move {
            let res = fut.await?;

            println!("Hi from response");

            Ok(res)
        })
    }
}

fn extract_token(req: &ServiceRequest) -> Option<String> {
    req.headers()
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(|s| s.to_string())
}
