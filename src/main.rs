#![recursion_limit = "1024"]
use crate::pb::hold_server::HoldServer;
use crate::util::make_rpc_path;
use crate::util::Holdstate;
use anyhow::{anyhow, Context, Result};
use cln_plugin::Plugin;
use cln_plugin::{options, Builder};
use log::{debug, info, warn};
use model::PluginState;
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use crate::hold::{hold_invoice, hold_invoice_cancel, hold_invoice_lookup, hold_invoice_settle};

mod hold;
mod hooks;
mod model;
mod tasks;
mod tls;
mod util;

pub mod pb;
mod server;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    debug!("Starting grpc plugin");
    std::env::set_var("CLN_PLUGIN_LOG", "cln_plugin=info,cln_rpc=info,debug");
    // let path = Path::new("lightning-rpc");

    let directory = std::env::current_dir()?;
    let (identity, ca_cert) = tls::init(&directory)?;

    let state = PluginState {
        blockheight: Arc::new(Mutex::new(u32::default())),
        holdinvoices: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
        identity,
        ca_cert,
    };

    let plugin = match Builder::new(tokio::io::stdin(), tokio::io::stdout())
        .option(options::ConfigOption::new(
            "grpc-hold-port",
            options::Value::Integer(-1),
            "Which port should the grpc plugin listen for incoming connections?",
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
            // match config::read_config(&p, state.clone()).await {
            //     Ok(()) => &(),
            //     Err(e) => return p.disable(format!("{}", e).as_str()).await,
            // };
            p
        }
        None => return Ok(()),
    };

    let bind_port = match plugin.option("grpc-hold-port") {
        Some(options::Value::Integer(-1)) => {
            log::info!("`grpc-hold-port` option is not configured, exiting.");
            plugin
                .disable("`grpc-hold-port` option is not configured.")
                .await?;
            return Ok(());
        }
        Some(options::Value::Integer(i)) => i,
        None => return Err(anyhow!("Missing 'grpc-hold-port' option")),
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

    let bind_addr: SocketAddr = format!("0.0.0.0:{}", bind_port).parse().unwrap();
    let rpc_path = make_rpc_path(confplugin.clone());

    tokio::select! {
        _ = confplugin.join() => {
        // This will likely never be shown, if we got here our
        // parent process is exiting and not processing out log
        // messages anymore.
            debug!("Plugin loop terminated")
        }
        e = run_interface(bind_addr,rpc_path, confplugin.clone()) => {
            warn!("Error running grpc interface: {:?}", e)
        }
    }
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
                .context("creating NodeServer instance")?,
        ))
        .serve(bind_addr);

    debug!(
        "Connecting to {:?} and serving grpc on {:?}",
        rpc_path, &bind_addr
    );

    server.await.context("serving requests")?;

    Ok(())
}
