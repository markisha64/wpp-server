use std::env;

use actix_web::rt::signal;
use actix_web::web;
use tokio::spawn;
use tokio::sync::watch;
use tokio::sync::watch::Receiver;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use tracing::info;

#[derive(Clone)]
pub struct Announcer {
    rx: Receiver<String>,
}

impl Announcer {
    pub async fn get_ip() -> anyhow::Result<String> {
        match env::var("HOST_URL") {
            Ok(s) => Ok(s),
            Err(_) => Ok(reqwest::get("https://api.ipify.org").await?.text().await?),
        }
    }

    pub async fn new() -> anyhow::Result<(web::Data<Announcer>, JoinHandle<()>)> {
        let my_ip = Announcer::get_ip().await?;
        let (tx, rx) = watch::channel(my_ip);

        let fut = spawn(async move {
            loop {
                tokio::select! {
                    _ = sleep(Duration::from_secs(300)) => {
                        if let Ok(my_ip) = Announcer::get_ip().await {
                            let _ = tx.send(my_ip);
                        }
                    }

                    evt = signal::ctrl_c() => {
                        evt.expect("failed to listen for ctrl-c");

                        info!("Gracefully shutting down Announcer");

                        break;
                    }
                }
            }
        });

        Ok((web::Data::new(Announcer { rx }), fut))
    }

    pub fn current_ip(&self) -> String {
        self.rx.borrow().clone()
    }
}
