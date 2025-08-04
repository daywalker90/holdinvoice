use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Error};

use cln_plugin::Plugin;
use cln_rpc::ClnRpc;
use lightning_invoice::Bolt11Invoice;
use serde_json::json;
use tokio::time::{self, Instant};

use crate::model::{Holdstate, PluginState};
use crate::rpc::{del_datastore_hold_invoice, listdatastore_all};
use crate::util::make_rpc_path;

pub async fn autoclean_holdinvoice_db(plugin: Plugin<PluginState>) -> Result<(), Error> {
    let rpc_path = make_rpc_path(plugin.clone());
    let mut rpc = ClnRpc::new(&rpc_path).await?;

    let (autoclean_cycle, _autoclean_paidinvoices_age, _autoclean_expiredinvoices_age) =
        check_autoclean_configs(&mut rpc).await?;
    time::sleep(autoclean_cycle).await;

    loop {
        log::info!("Starting autoclean_holdinvoice_db");
        let now = Instant::now();
        let unix_now = SystemTime::now().duration_since(UNIX_EPOCH)?;
        let (autoclean_cycle, autoclean_paidinvoices_age, autoclean_expiredinvoices_age) =
            check_autoclean_configs(&mut rpc).await?;

        if autoclean_expiredinvoices_age.is_zero() && autoclean_paidinvoices_age.is_zero() {
            log::info!("autoclean_holdinvoice_db: paid and expired invoice cleanup disabled");
            time::sleep(autoclean_cycle).await;
        } else {
            let mut count_paid = 0;
            let mut count_expired = 0;

            let holdinvoices = listdatastore_all(&mut rpc).await?;
            for invoice in holdinvoices {
                let bolt11 = Bolt11Invoice::from_str(&invoice.bolt11)?;
                if invoice.state == Holdstate::Settled {
                    if autoclean_paidinvoices_age.is_zero() {
                        continue;
                    }
                    if let Some(p_at) = invoice.paid_at {
                        if unix_now.saturating_sub(Duration::from_secs(p_at))
                            > autoclean_paidinvoices_age
                        {
                            let _res = del_datastore_hold_invoice(
                                &mut rpc,
                                bolt11.payment_hash().to_string(),
                            )
                            .await;
                            count_paid += 1;
                        }
                    }
                } else {
                    if autoclean_expiredinvoices_age.is_zero() {
                        continue;
                    }
                    if let Some(expires_at) = bolt11.expires_at() {
                        if unix_now.saturating_sub(expires_at) > autoclean_expiredinvoices_age {
                            let _res = del_datastore_hold_invoice(
                                &mut rpc,
                                bolt11.payment_hash().to_string(),
                            )
                            .await;
                            count_expired += 1;
                        }
                    }
                }
            }

            log::info!(
                "autoclean_holdinvoice_db: cleaned up {} paid and {} expired holdinvoices in {}ms",
                count_paid,
                count_expired,
                now.elapsed().as_millis()
            );
            time::sleep(autoclean_cycle.saturating_sub(now.elapsed())).await;
        }
    }
}

async fn check_autoclean_configs(
    rpc: &mut ClnRpc,
) -> Result<(Duration, Duration, Duration), Error> {
    let listconfigs: serde_json::Value = rpc.call_raw("listconfigs", &json!({})).await?;
    let configs = listconfigs
        .get("configs")
        .ok_or_else(|| anyhow!("no configs"))?;
    let autoclean_cycle = configs
        .get("autoclean-cycle")
        .ok_or_else(|| anyhow!("no autoclean-cycle"))?
        .get("value_int")
        .ok_or_else(|| anyhow!("no autoclean-cycle value_int"))?
        .as_u64()
        .ok_or_else(|| anyhow!("autoclean-cycle value_int is not a number"))?;

    let autoclean_paidinvoices_age_config = configs.get("autoclean-paidinvoices-age");
    let autoclean_paidinvoices_age = if let Some(apia) = autoclean_paidinvoices_age_config {
        apia.get("value_int")
            .ok_or_else(|| anyhow!("no autoclean-paidinvoices-age value_int"))?
            .as_u64()
            .ok_or_else(|| anyhow!("autoclean-paidinvoices-age value_int is not a number"))?
    } else {
        0
    };

    let autoclean_expiredinvoices_age_config = configs.get("autoclean-expiredinvoices-age");
    let autoclean_expiredinvoices_age = if let Some(aeia) = autoclean_expiredinvoices_age_config {
        aeia.get("value_int")
            .ok_or_else(|| anyhow!("no autoclean-expiredinvoices-age value_int"))?
            .as_u64()
            .ok_or_else(|| anyhow!("autoclean-expiredinvoices-age value_int is not a number"))?
    } else {
        0
    };

    Ok((
        Duration::from_secs(autoclean_cycle),
        Duration::from_secs(autoclean_paidinvoices_age),
        Duration::from_secs(autoclean_expiredinvoices_age),
    ))
}
