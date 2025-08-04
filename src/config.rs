use anyhow::{anyhow, Error};
use cln_plugin::ConfiguredPlugin;

use crate::{
    errors::config_value_error, model::PluginState, OPT_CANCEL_HOLD_BEFORE_HTLC_EXPIRY_BLOCKS,
    OPT_GRPC_HOLD_PORT, OPT_HOLD_STARTUP_LOCK,
};

pub fn verify_config_options(
    plugin: &ConfiguredPlugin<PluginState, tokio::io::Stdin, tokio::io::Stdout>,
) -> Result<(), Error> {
    let cancel_hold_before_htlc_expiry_blocks =
        plugin.option(&OPT_CANCEL_HOLD_BEFORE_HTLC_EXPIRY_BLOCKS)?;
    if let Ok(b) = u32::try_from(cancel_hold_before_htlc_expiry_blocks) {
        if b == 0 {
            return Err(anyhow!(config_value_error(
                OPT_CANCEL_HOLD_BEFORE_HTLC_EXPIRY_BLOCKS.name,
                cancel_hold_before_htlc_expiry_blocks
            )));
        }
    } else {
        return Err(anyhow!(config_value_error(
            OPT_CANCEL_HOLD_BEFORE_HTLC_EXPIRY_BLOCKS.name,
            cancel_hold_before_htlc_expiry_blocks
        )));
    }

    if let Some(grpc_hold_port) = plugin.option(&OPT_GRPC_HOLD_PORT)? {
        if u16::try_from(grpc_hold_port).is_err() {
            return Err(anyhow!(config_value_error(
                OPT_GRPC_HOLD_PORT.name,
                grpc_hold_port
            )));
        }
    };

    let hold_startup_lock = plugin.option(&OPT_HOLD_STARTUP_LOCK)?;
    if u64::try_from(hold_startup_lock).is_err() {
        return Err(anyhow!(config_value_error(
            OPT_HOLD_STARTUP_LOCK.name,
            hold_startup_lock
        )));
    }

    Ok(())
}
