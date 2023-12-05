#![recursion_limit = "1024"]
use crate::model::Holdstate;
use crate::pb::hold_server::HoldServer;
use crate::util::make_rpc_path;
use anyhow::{anyhow, Context, Result};
use cln_plugin::Plugin;
use cln_plugin::{options, Builder};
use log::{debug, info, warn};
use model::{Config, PluginState};
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use crate::hold::{hold_invoice, hold_invoice_cancel, hold_invoice_lookup, hold_invoice_settle};

mod config;
mod errors;
mod hold;
mod hooks;
mod model;
mod rpc;
mod tasks;
mod tls;
mod util;

pub mod pb;
mod server;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    debug!("Starting grpc plugin");
    std::env::set_var("CLN_PLUGIN_LOG", "cln_plugin=info,cln_rpc=info,debug");

    let directory = std::env::current_dir()?;
    let (identity, ca_cert) = tls::init(&directory)?;

    let state = PluginState {
        blockheight: Arc::new(Mutex::new(u32::default())),
        holdinvoices: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
        config: Arc::new(Mutex::new(Config::new())),
        identity,
        ca_cert,
    };
    let defaultconfig = Config::new();

    let plugin = match Builder::new(tokio::io::stdin(), tokio::io::stdout())
        .option(options::ConfigOption::new(
            "grpc-hold-port",
            options::Value::Integer(-1),
            "Which port should the grpc plugin listen for incoming connections?",
        ))
        .option(options::ConfigOption::new(
            &defaultconfig.cancel_hold_before_htlc_expiry_blocks.0,
            options::Value::OptInteger,
            "Number of blocks before expiring htlcs get auto-canceled and invoice is canceled",
        ))
        .option(options::ConfigOption::new(
            &defaultconfig.cancel_hold_before_invoice_expiry_seconds.0,
            options::Value::OptInteger,
            "Seconds before invoice expiry when an invoice and pending htlcs get auto-canceled",
        ))
        .rpcmethod(
            "holdinvoice",
            "create a new invoice and hold it",
            hold_invoice,
        )
        .rpcmethod(
            "holdinvoicesettle",
            "settle htlcs to corresponding holdinvoice",
            hold_invoice_settle,
        )
        .rpcmethod(
            "holdinvoicecancel",
            "cancel htlcs to corresponding holdinvoice",
            hold_invoice_cancel,
        )
        .rpcmethod(
            "holdinvoicelookup",
            "lookup hold status of holdinvoice",
            hold_invoice_lookup,
        )
        .hook("htlc_accepted", hooks::htlc_handler)
        .subscribe("block_added", hooks::block_added)
        .configure()
        .await?
    {
        Some(p) => {
            info!("read config");
            match config::read_config_options(&p, state.clone()) {
                Ok(()) => &(),
                Err(e) => return p.disable(format!("{}", e).as_str()).await,
            };
            p
        }
        None => return Ok(()),
    };

    let bind_port = match plugin.option("grpc-hold-port") {
        Some(options::Value::Integer(-1)) => {
            log::info!(
                "`grpc-hold-port` option is not configured, gRPC server will not bind to a port."
            );
            None
        }
        Some(options::Value::Integer(i)) => Some(i),
        None => {
            log::info!(
                "`grpc-hold-port` option not provided, gRPC server will not bind to a port."
            );
            None
        }
        Some(o) => return Err(anyhow!("grpc-hold-port is not a valid integer: {:?}", o)),
    };
    let confplugin;
    match plugin.start(state.clone()).await {
        Ok(p) => {
            info!("starting lookup_state task");
            confplugin = p;
            let cleanupclone = confplugin.clone();
            tokio::spawn(async move {
                match tasks::autoclean_holdinvoice_db(cleanupclone).await {
                    Ok(()) => (),
                    Err(e) => warn!(
                        "Error in autoclean_holdinvoice_db thread: {}",
                        e.to_string()
                    ),
                };
            });
        }
        Err(e) => return Err(anyhow!("Error starting plugin: {}", e)),
    }

    if let Some(port) = bind_port {
        let bind_addr: SocketAddr = format!("0.0.0.0:{}", port).parse().unwrap();
        let rpc_path = make_rpc_path(confplugin.clone());
        tokio::spawn(run_interface(bind_addr, rpc_path, confplugin.clone()));
    }

    confplugin.join().await.unwrap_or_else(|e| {
        warn!("Error joining holdinvoice plugin: {}", e);
    });
    Ok(())
}

async fn run_interface(
    bind_addr: SocketAddr,
    rpc_path: PathBuf,
    plugin: Plugin<PluginState>,
) -> Result<()> {
    let identity = plugin.state().identity.to_tonic_identity();
    let ca_cert = tonic::transport::Certificate::from_pem(plugin.state().ca_cert.clone());

    let tls = tonic::transport::ServerTlsConfig::new()
        .identity(identity)
        .client_ca_root(ca_cert);

    let server = tonic::transport::Server::builder()
        .tls_config(tls)
        .context("configuring tls")?
        .add_service(HoldServer::new(
            server::Server::new(&rpc_path, plugin.clone())
                .await
                .context("creating HoldServer instance")?,
        ))
        .serve(bind_addr);

    debug!(
        "Connecting to {:?} and serving grpc on {:?}",
        rpc_path, &bind_addr
    );

    server.await.context("serving requests")?;

    Ok(())
}
