mod lcu;
mod ws;

use actix_web::{App, HttpRequest, HttpResponse, HttpServer, web};
use std::sync::Arc;
use tokio::sync::broadcast;

async fn ws_route(
    req: HttpRequest,
    stream: web::Payload,
    tx: web::Data<Arc<broadcast::Sender<String>>>,
) -> Result<HttpResponse, actix_web::Error> {
    ws::start(&req, stream, tx.subscribe())
}

const IP: &str = "127.0.0.1";
const PORT: u16 = 2009;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt().init();

    if rustls::crypto::ring::default_provider()
        .install_default()
        .is_err()
    {
        tracing::error!("Failed to install the ring rustls crypto provider");
        std::process::exit(1);
    }

    let (tx, _) = broadcast::channel::<String>(32);
    let tx_arc = Arc::new(tx);

    lcu::spawn_lcu_listener((*tx_arc).clone());

    tracing::info!("Starting Overlay Server on {}:{}", IP, PORT);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(Arc::clone(&tx_arc)))
            .route("/ws", web::get().to(ws_route))
    })
    .workers(1)
    .bind((IP, PORT))?
    .run()
    .await
}
