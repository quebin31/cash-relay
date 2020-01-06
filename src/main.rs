#[macro_use]
extern crate clap;
#[macro_use]
extern crate log;
#[macro_use]
extern crate serde;

pub mod bitcoin;
pub mod crypto;
pub mod db;
pub mod models;
pub mod net;
pub mod settings;

use std::io;

use actix_cors::Cors;
use actix_web::{http::header, middleware::Logger, web, App, HttpServer};
use env_logger::Env;
use lazy_static::lazy_static;

use crate::{
    bitcoin::{BitcoinClient, WalletState},
    db::Database,
    net::{payments::*, *},
    settings::Settings,
};

lazy_static! {
    pub static ref SETTINGS: Settings = Settings::new().expect("couldn't load config");
}

#[actix_rt::main]
async fn main() -> io::Result<()> {
    // Init logging
    env_logger::from_env(Env::default().default_filter_or("actix_web=info,keyserver=info")).init();
    info!("starting server @ {}", SETTINGS.bind);

    // Open DB
    let db = Database::try_new(&SETTINGS.db_path).expect("failed to open database");

    // Init wallet
    let wallet_state = WalletState::default();

    // Init Bitcoin client
    let bitcoin_client = BitcoinClient::new(
        format!("http://{}:{}", SETTINGS.node_ip.clone(), SETTINGS.rpc_port),
        SETTINGS.rpc_username.clone(),
        SETTINGS.rpc_password.clone(),
    );

    // Init REST server
    HttpServer::new(move || {
        let db_inner = db.clone();
        let wallet_state_inner = wallet_state.clone();
        let bitcoin_client_inner = bitcoin_client.clone();

        // Init CORs
        let cors = Cors::new()
            .allowed_methods(vec!["GET", "PUT", "POST"])
            .allowed_headers(vec![header::AUTHORIZATION, header::CONTENT_TYPE])
            .expose_headers(vec![
                header::AUTHORIZATION,
                header::ACCEPT,
                header::LOCATION,
            ])
            .finish();

        // Init app
        App::new()
            .wrap(Logger::default())
            .wrap(Logger::new("%a %{User-Agent}i"))
            .wrap(cors)
            .service(
                // Address scope
                web::scope("/{addr}").service(
                    // Message handlers
                    web::resource("/message")
                        .data(db_inner.clone())
                        .route(web::get().to(get_messages))
                        .route(web::put().to(put_message)),
                ).service(
                    // Filter methods
                    web::resource("/filter")
                        .data(db_inner)
                        .wrap(CheckPayment::new(
                            bitcoin_client_inner.clone(),
                            wallet_state_inner.clone(),
                        )) // Apply payment check to put key
                        .route(web::get().to(get_filter))
                        .route(web::put().to(put_filter))
                )
            )
            .service(
                // Payment endpoint
                web::resource("/payments")
                    .data((bitcoin_client_inner, wallet_state_inner))
                    .route(web::post().to(payment_handler)),
            )
            .service(actix_files::Files::new("/", "./static/").index_file("index.html"))
    })
    .bind(&SETTINGS.bind)?
    .run()
    .await
}
