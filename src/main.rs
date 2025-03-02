#![recursion_limit = "1024"]
use crate::model::Holdstate;
use crate::pb::hold_server::HoldServer;
use crate::util::make_rpc_path;
use anyhow::{anyhow, Context, Result};
use cln_plugin::options::{ConfigOption, DefaultIntegerConfigOption, IntegerConfigOption};
use cln_plugin::Plugin;
use cln_plugin::{Builder, ConfiguredPlugin};
use cln_rpc::ClnRpc;
use log::{debug, info, warn};
use model::{PluginState, HOLD_STARTUP_LOCK};
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::error::Error;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tls::do_certificates_exist;
use tokio::time;

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

const OPT_GRPC_HOLD_PORT: IntegerConfigOption = ConfigOption::new_i64_no_default(
    "grpc-hold-port",
    "Which port should the grpc plugin listen for incoming connections?",
);
const OPT_CANCEL_HOLD_BEFORE_HTLC_EXPIRY_BLOCKS: DefaultIntegerConfigOption =
    ConfigOption::new_i64_with_default(
        "holdinvoice-cancel-before-htlc-expiry",
        6,
        "Number of blocks before expiring htlcs get auto-canceled and invoice is canceled",
    );
const OPT_CANCEL_HOLD_BEFORE_INVOICE_EXPIRY_SECONDS: DefaultIntegerConfigOption =
    ConfigOption::new_i64_with_default(
        "holdinvoice-cancel-before-invoice-expiry",
        1_800,
        "Seconds before invoice expiry when an invoice and pending htlcs get auto-canceled",
    );

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), anyhow::Error> {
    std::env::set_var(
        "CLN_PLUGIN_LOG",
        "cln_plugin=info,cln_rpc=info,cln_grpc=info,holdinvoice=debug,warn",
    );

    let plugin = match Builder::new(tokio::io::stdin(), tokio::io::stdout())
        .option(OPT_GRPC_HOLD_PORT)
        .option(OPT_CANCEL_HOLD_BEFORE_HTLC_EXPIRY_BLOCKS)
        .option(OPT_CANCEL_HOLD_BEFORE_INVOICE_EXPIRY_SECONDS)
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
            info!("verifying config options");
            match config::verify_config_options(&p) {
                Ok(()) => (),
                Err(e) => {
                    log_error(e.to_string());
                    return Err(e);
                }
            };
            p
        }
        None => return Err(anyhow!("Error configuring the plugin!")),
    };

    let bind_port = match plugin.option(&OPT_GRPC_HOLD_PORT)? {
        Some(port) => Some(port),
        None => {
            log::info!(
                "`grpc-hold-port` option not provided, gRPC server will not bind to a port."
            );
            None
        }
    };

    let state = match init_plugin_state(&plugin).await {
        Ok(s) => s,
        Err(e) => {
            log_error(e.to_string());
            return Err(e);
        }
    };

    let confplugin;
    match plugin.start(state.clone()).await {
        Ok(p) => {
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
        let bind_addr: SocketAddr = format!("0.0.0.0:{}", port).parse()?;
        let rpc_path = make_rpc_path(confplugin.clone());
        let grpc_plugin_clone = confplugin.clone();
        tokio::spawn(async move {
            match run_interface(bind_addr, rpc_path, grpc_plugin_clone).await {
                Ok(_) => log::info!("grpc interface stopped"),
                Err(e) => log::warn!("{}", e.to_string()),
            }
        });
    }

    time::sleep(Duration::from_secs(HOLD_STARTUP_LOCK)).await;
    *confplugin.state().startup_lock.lock() = false;

    confplugin.join().await
}

async fn init_plugin_state(
    plugin: &ConfiguredPlugin<PluginState, tokio::io::Stdin, tokio::io::Stdout>,
) -> Result<PluginState, anyhow::Error> {
    let directory = std::env::current_dir()?;
    let max_retries = 10;
    let mut retries = 0;
    while retries < max_retries && !do_certificates_exist(&directory) {
        log::debug!("Certificates incomplete. Retrying...");
        time::sleep(Duration::from_millis(500)).await;
        retries += 1;
    }

    let (identity, ca_cert) = tls::init(&directory)?;

    let rpc_path =
        Path::new(&plugin.configuration().lightning_dir).join(plugin.configuration().rpc_file);
    let rpc = ClnRpc::new(&rpc_path).await?;

    Ok(PluginState {
        blockheight: Arc::new(Mutex::new(u32::default())),
        holdinvoices: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
        identity,
        ca_cert,
        startup_lock: Arc::new(Mutex::new(true)),
        rpc: Arc::new(tokio::sync::Mutex::new(rpc)),
    })
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

    server
        .await
        .map_err(|e| anyhow!("Error serving grpc: {} {:?}", e.to_string(), e.source()))
}

fn log_error(error: String) {
    println!(
        "{}",
        serde_json::json!({"jsonrpc": "2.0",
                          "method": "log",
                          "params": {"level":"warn", "message":error}})
    );
}
