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

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // 1. Setup Security/Crypto
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    // 2. Setup Broadcast Channel
    let (tx, _) = broadcast::channel::<String>(32);
    let tx_arc = Arc::new(tx);

    // 3. Start LCU Listener
    lcu::spawn_lcu_listener(Arc::clone(&tx_arc));

    println!("Overlay Server: ws://127.0.0.1:8080/ws");

    // 4. Start Web Server
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(Arc::clone(&tx_arc)))
            .route("/ws", web::get().to(ws_route))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
