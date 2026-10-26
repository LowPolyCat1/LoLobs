mod config;
mod lcu;
mod message;
mod session;
mod snapshot;
mod web_dir;
mod ws;

use actix_web::{App, HttpRequest, HttpResponse, HttpServer, web};
use clap::Parser;
use config::{Config, Overrides};
use session::SessionStore;
use snapshot::Outbox;
use std::net::SocketAddr;
use std::path::Path;

async fn ws_route(
    req: HttpRequest,
    stream: web::Payload,
    outbox: web::Data<Outbox>,
) -> Result<HttpResponse, actix_web::Error> {
    ws::start(&req, stream, outbox.subscribe(), outbox.snapshot())
}

/// Prints help and version text and exits. Logs other errors and exits with code 1.
fn parse_command_line() -> Overrides {
    match Overrides::try_parse() {
        Ok(overrides) => overrides,
        Err(err) if !err.use_stderr() => err.exit(),
        Err(err) => {
            tracing::error!("Invalid command line: {}", err);
            std::process::exit(1);
        }
    }
}

fn load_config(exe_path: Option<&Path>, cli: Overrides) -> Config {
    config::load(exe_path, cli).unwrap_or_else(|err| {
        tracing::error!("{}", err);
        std::process::exit(1);
    })
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt().init();

    let exe_path = std::env::current_exe().ok();
    let config = load_config(exe_path.as_deref(), parse_command_line());
    let address = SocketAddr::new(config.ip, config.port);

    if rustls::crypto::ring::default_provider()
        .install_default()
        .is_err()
    {
        tracing::error!("Failed to install the ring rustls crypto provider");
        std::process::exit(1);
    }

    let outbox = Outbox::new(32);

    lcu::spawn_lcu_listener(
        outbox.clone(),
        lcu::Selection::new(config.queue, config.match_limit),
        SessionStore::open(config.session, exe_path.as_deref()),
    );

    tracing::info!("Starting Overlay Server on {}", address);

    let web_dir = web_dir::resolve();
    if let Some(dir) = &web_dir {
        web_dir::log_page_urls(dir, address);
    }

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(outbox.clone()))
            .route("/ws", web::get().to(ws_route))
            .configure(|cfg| web_dir::configure(cfg, web_dir.as_deref()))
    })
    .workers(1)
    .bind(address)?
    .run()
    .await
}
