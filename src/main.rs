mod actor;
mod lcu;
mod messages;

use actix_web::{App, HttpRequest, HttpServer, Responder, web};
use actix_web_actors::ws;
use std::sync::Arc;
use tokio::sync::broadcast;

async fn ws_route(
    req: HttpRequest,
    stream: web::Payload,
    tx: web::Data<Arc<broadcast::Sender<String>>>,
) -> impl Responder {
    ws::start(
        actor::MyWs {
            receiver: tx.subscribe(),
        },
        &req,
        stream,
    )
}

const IP: &str = "127.0.0.1";
const PORT: u16 = 2009;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt().init();

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let (tx, _) = broadcast::channel::<String>(32);
    let tx_arc = Arc::new(tx);

    lcu::spawn_lcu_listener((*tx_arc).clone());

    tracing::info!("Starting Overlay Server on {}:{}", IP, PORT);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(Arc::clone(&tx_arc)))
            .route("/ws", web::get().to(ws_route))
    })
    .bind((IP, PORT))?
    .run()
    .await
}
