use anyhow::{anyhow, Error};
use cln_plugin::{options, ConfiguredPlugin};

use crate::{errors::config_value_error, model::PluginState};

pub fn read_config_options(
    plugin: &ConfiguredPlugin<PluginState, tokio::io::Stdin, tokio::io::Stdout>,
    state: PluginState,
) -> Result<(), Error> {
    let mut config = state.config.lock();
    if let Some(options::Value::Integer(b)) =
        plugin.option(&config.cancel_hold_before_htlc_expiry_blocks.0)
    {
        if b > 0 {
            config.cancel_hold_before_htlc_expiry_blocks.1 = b as u32;
        } else {
            return Err(anyhow!(config_value_error(
                &config.cancel_hold_before_htlc_expiry_blocks.0,
                0
            )));
        }
    }

    if let Some(options::Value::Integer(b)) =
        plugin.option(&config.cancel_hold_before_invoice_expiry_seconds.0)
    {
        if b > 0 {
            config.cancel_hold_before_invoice_expiry_seconds.1 = b as u64;
        } else {
            return Err(anyhow!(config_value_error(
                &config.cancel_hold_before_invoice_expiry_seconds.0,
                0
            )));
        }
    }
    Ok(())
}
