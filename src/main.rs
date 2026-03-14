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
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let (tx, _) = broadcast::channel::<String>(32);
    let tx_arc = Arc::new(tx);

    lcu::spawn_lcu_listener(Arc::clone(&tx_arc));

    println!("-------------------------------------------");
    println!("🚀 Rank Overlay Server Started!");
    println!("WebSocket URL: ws://127.0.0.1:8080/ws");
    println!("Open overlay.html in your browser or OBS.");
    println!("-------------------------------------------");

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(Arc::clone(&tx_arc)))
            .route("/ws", web::get().to(ws_route))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
