//! Start a localtunnel server,
//! request a proxy endpoint at `domain.tld/<your-endpoint>`,
//! user's request then proxied via `<your-endpoint>.domain.tld`.

use std::sync::OnceLock;
use std::time::Duration;
use std::{net::SocketAddr, sync::Arc};

use actix_web::{web, App, HttpServer};
use anyhow::Result;
use hyper::{server::conn::http1, service::service_fn};
use tokio::{net::TcpListener, sync::Mutex, time::timeout};

use crate::api::{api_status, request_endpoint};
use crate::config::Config;
use crate::proxy::proxy_handler;
use crate::state::{ClientManager, State};

mod api;
mod auth;
mod config;
mod error;
mod proxy;
mod state;

/// The interval between cleanup checks
const CLEANUP_CHECK_INTERVAL: Duration = Duration::from_secs(60);

static ENV_CONFIG: OnceLock<Config> = OnceLock::new();

pub(crate) fn env_config() -> &'static Config {
    ENV_CONFIG.get_or_init(|| {
        dotenvy::dotenv().ok();
        envy::from_env::<Config>().unwrap_or_default()
    })
}

pub struct ServerConfig {
    pub domain: String,
    pub api_port: u16,
    pub secure: bool,
    pub max_sockets: u8,
    pub proxy_port: u16,
    pub require_auth: bool,
    /// When both are `Some`, tunnel clients connect to a port in this inclusive range
    /// (easier to expose through firewalls / Docker than ephemeral `0.0.0.0:0`).
    /// When either is `None`, use ephemeral ports (legacy behaviour).
    pub client_connect_port_min: Option<u16>,
    pub client_connect_port_max: Option<u16>,
}

/// Start the proxy use low level api from hyper.
/// Proxy endpoint request is served via actix-web.
pub async fn start(config: ServerConfig) -> Result<()> {
    let ServerConfig {
        domain,
        api_port,
        secure,
        max_sockets,
        proxy_port,
        require_auth,
        client_connect_port_min,
        client_connect_port_max,
    } = config;

    let client_port_range = match (client_connect_port_min, client_connect_port_max) {
        (None, None) => None,
        (Some(min), Some(max)) if min <= max => Some(min..=max),
        (Some(min), Some(max)) => {
            anyhow::bail!(
                "client_connect_port_min ({min}) must be <= client_connect_port_max ({max})"
            )
        }
        _ => anyhow::bail!(
            "set both client_connect_port_min and client_connect_port_max, or neither (ephemeral ports)"
        ),
    };

    log::info!("Api server listens at {} {}", &domain, api_port);
    log::info!(
        "Start proxy server at {} {}, options: {} {}, require auth: {}",
        &domain,
        proxy_port,
        secure,
        max_sockets,
        require_auth
    );
    match &client_port_range {
        Some(r) => log::info!("Tunnel client TCP listeners use port range {r:?} (inclusive)"),
        None => log::info!("Tunnel client TCP listeners use ephemeral ports (OS-assigned)"),
    }

    let manager = Arc::new(Mutex::new(ClientManager::new(max_sockets, client_port_range)));
    let api_state = web::Data::new(State {
        manager: manager.clone(),
        max_sockets,
        require_auth,
        secure,
        domain,
    });

    let proxy_addr: SocketAddr = ([0, 0, 0, 0], proxy_port).into();
    let listener = TcpListener::bind(proxy_addr).await?;
    tokio::spawn(async move {
        loop {
            match timeout(CLEANUP_CHECK_INTERVAL, listener.accept()).await {
                Ok(Ok((stream, _))) => {
                    log::info!("Accepted a new proxy request");

                    let proxy_manager = manager.clone();
                    let service = service_fn(move |req| proxy_handler(req, proxy_manager.clone()));

                    tokio::spawn(async move {
                        if let Err(err) = http1::Builder::new()
                            .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                            .with_upgrades()
                            .await
                        {
                            log::error!("Failed to serve connection: {:?}", err);
                        }
                    });
                }
                Ok(Err(e)) => log::error!("Failed to accept the request: {:?}", e),
                Err(_) => {
                    // timeout, cleanup old connections
                    let mut manager = manager.lock().await;
                    manager.cleanup().await;
                }
            }
        }
    });

    HttpServer::new(move || {
        App::new()
            .app_data(api_state.clone())
            .service(api_status)
            .service(request_endpoint)
    })
    .bind(("0.0.0.0", api_port))?
    .run()
    .await?;

    Ok(())
}
