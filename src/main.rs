mod lcu;
mod snapshot;
mod ws;

use actix_web::{App, HttpRequest, HttpResponse, HttpServer, web};
use snapshot::Outbox;

async fn ws_route(
    req: HttpRequest,
    stream: web::Payload,
    outbox: web::Data<Outbox>,
) -> Result<HttpResponse, actix_web::Error> {
    ws::start(&req, stream, outbox.subscribe(), outbox.snapshot())
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

    let outbox = Outbox::new(32);

    lcu::spawn_lcu_listener(outbox.clone());

    tracing::info!("Starting Overlay Server on {}:{}", IP, PORT);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(outbox.clone()))
            .route("/ws", web::get().to(ws_route))
    })
    .workers(1)
    .bind((IP, PORT))?
    .run()
    .await
}
