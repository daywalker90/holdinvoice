use anyhow::{anyhow, Error};
use cln_plugin::ConfiguredPlugin;

use crate::{
    errors::config_value_error, model::PluginState, OPT_CANCEL_HOLD_BEFORE_HTLC_EXPIRY_BLOCKS,
    OPT_CANCEL_HOLD_BEFORE_INVOICE_EXPIRY_SECONDS,
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

    let cancel_hold_before_invoice_expiry_seconds =
        plugin.option(&OPT_CANCEL_HOLD_BEFORE_INVOICE_EXPIRY_SECONDS)?;
    if let Ok(i) = u64::try_from(cancel_hold_before_invoice_expiry_seconds) {
        if i == 0 {
            return Err(anyhow!(config_value_error(
                OPT_CANCEL_HOLD_BEFORE_INVOICE_EXPIRY_SECONDS.name,
                cancel_hold_before_invoice_expiry_seconds
            )));
        }
    } else {
        return Err(anyhow!(config_value_error(
            OPT_CANCEL_HOLD_BEFORE_INVOICE_EXPIRY_SECONDS.name,
            cancel_hold_before_invoice_expiry_seconds
        )));
    }
    Ok(())
}
