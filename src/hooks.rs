use std::{
    collections::HashMap,
    str::FromStr,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, Error};
use cln_plugin::Plugin;
use cln_rpc::{
    model::{requests::ListinvoicesRequest, responses::ListinvoicesInvoices},
    primitives::{Amount, ShortChannelId},
    ClnRpc,
};
use log::{debug, info, warn};
use serde_json::json;
use tokio::time::{self};

use crate::util::{cleanup_pluginstate_holdinvoices, make_rpc_path};
use crate::{
    model::{HoldHtlc, HoldInvoice, HtlcIdentifier, PluginState},
    rpc::{datastore_update_state, listdatastore_state},
    OPT_CANCEL_HOLD_BEFORE_HTLC_EXPIRY_BLOCKS, OPT_CANCEL_HOLD_BEFORE_INVOICE_EXPIRY_SECONDS,
};
use crate::{rpc::datastore_htlc_expiry, Holdstate};

const WIRE_INCORRECT_OR_UNKNOWN_PAYMENT_DETAILS: &str = "400F";

pub async fn htlc_handler(
    plugin: Plugin<PluginState>,
    v: serde_json::Value,
) -> Result<serde_json::Value, Error> {
    if let Some(htlc) = v.get("htlc") {
        if let Some(pay_hash) = htlc
            .get("payment_hash")
            .and_then(|pay_hash| pay_hash.as_str())
        {
            debug!("payment_hash: `{}`. htlc_hook started!", pay_hash);
            let rpc_path = make_rpc_path(plugin.clone());
            let mut rpc = ClnRpc::new(&rpc_path).await?;

            let is_new_invoice;
            let cltv_expiry;

            let amount_msat;

            let invoice;
            let scid;
            let chan_htlc_id;
            let global_htlc_ident;
            let hold_state;

            {
                let mut holdinvoices = plugin.state().holdinvoices.lock().await;
                let generation;
                if let Some(holdinvoice) = holdinvoices.get_mut(&pay_hash.to_string()) {
                    is_new_invoice = false;
                    debug!(
                        "payment_hash: `{}`. Htlc is for a \
                        known holdinvoice! Processing...",
                        pay_hash
                    );

                    hold_state = holdinvoice.hold_state;
                    invoice = holdinvoice.invoice.clone();
                    generation = holdinvoice.generation;
                } else {
                    is_new_invoice = true;
                    debug!(
                        "payment_hash: `{}`. New htlc, \
                        checking if it's our invoice...",
                        pay_hash
                    );

                    match listdatastore_state(&mut rpc, pay_hash.to_string()).await {
                        Ok(dbstate) => {
                            debug!(
                                "payment_hash: `{}`. Htlc is for a \
                                holdinvoice! Processing...",
                                pay_hash
                            );
                            hold_state = Holdstate::from_str(&dbstate.string.unwrap())?;
                            generation = dbstate.generation.unwrap_or(0);

                            invoice = rpc
                                .call_typed(&ListinvoicesRequest {
                                    index: None,
                                    invstring: None,
                                    label: None,
                                    limit: None,
                                    offer_id: None,
                                    payment_hash: Some(pay_hash.to_string()),
                                    start: None,
                                })
                                .await?
                                .invoices
                                .first()
                                .ok_or(anyhow!(
                                    "payment_hash: `{}`. holdinvoice not found!",
                                    pay_hash
                                ))?
                                .clone();
                        }
                        Err(_e) => {
                            debug!(
                                "payment_hash: `{}`. Not a holdinvoice! \
                                Continue...",
                                pay_hash
                            );
                            return Ok(json!({"result": "continue"}));
                        }
                    };
                }

                chan_htlc_id = if let Some(chid) = htlc.get("id") {
                    chid.as_u64().unwrap()
                } else {
                    warn!(
                        "payment_hash: `{}`. htlc id not found! \
                        Rejecting htlc...",
                        pay_hash
                    );
                    return Ok(json!({"result": "fail",
                        "failure_message": get_failure_message(*plugin.state().blockheight.lock(),0)
                    }));
                };

                scid = if let Some(id) = htlc.get("short_channel_id") {
                    ShortChannelId::from_str(id.as_str().unwrap())?
                } else {
                    warn!(
                        "payment_hash: `{}`. short_channel_id not found! \
                        Rejecting htlc...",
                        pay_hash
                    );
                    return Ok(json!({"result": "fail",
                        "failure_message": get_failure_message(*plugin.state().blockheight.lock(),0)
                    }));
                };

                global_htlc_ident = HtlcIdentifier {
                    scid,
                    htlc_id: chan_htlc_id,
                };

                cltv_expiry = if let Some(ce) = htlc.get("cltv_expiry") {
                    ce.as_u64().unwrap() as u32
                } else {
                    warn!(
                        "payment_hash: `{}`. cltv_expiry not found! \
                        Rejecting htlc...",
                        pay_hash
                    );
                    return Ok(json!({"result": "fail",
                        "failure_message": get_failure_message(*plugin.state().blockheight.lock(),0)
                    }));
                };

                amount_msat = if let Some(amt) = htlc.get("amount_msat") {
                    amt.as_u64().unwrap()
                } else {
                    warn!(
                        "payment_hash: `{}` scid: `{}` htlc_id: {}: \
                            amount_msat not found! Rejecting htlc...",
                        pay_hash, global_htlc_ident.scid, global_htlc_ident.htlc_id
                    );
                    return Ok(json!({"result": "fail",
                        "failure_message": get_failure_message(*plugin.state().blockheight.lock(),0)
                    }));
                };

                if is_new_invoice {
                    datastore_htlc_expiry(&mut rpc, pay_hash.to_string(), cltv_expiry.to_string())
                        .await?;

                    let mut htlc_data = HashMap::new();
                    htlc_data.insert(
                        global_htlc_ident.clone(),
                        HoldHtlc {
                            amount_msat,
                            cltv_expiry,
                            loop_mutex: Arc::new(tokio::sync::Mutex::new(true)),
                        },
                    );
                    holdinvoices.insert(
                        pay_hash.to_string(),
                        HoldInvoice {
                            hold_state,
                            generation,
                            htlc_data,
                            last_htlc_expiry: cltv_expiry,
                            invoice: invoice.clone(),
                        },
                    );
                } else {
                    let holdinvoice = holdinvoices.get_mut(&pay_hash.to_string()).unwrap();
                    holdinvoice.htlc_data.insert(
                        global_htlc_ident.clone(),
                        HoldHtlc {
                            amount_msat,
                            cltv_expiry,
                            loop_mutex: Arc::new(tokio::sync::Mutex::new(true)),
                        },
                    );

                    let earliest_htlc_expiry = holdinvoice
                        .htlc_data
                        .values()
                        .map(|htlc| htlc.cltv_expiry)
                        .min()
                        .unwrap();

                    if holdinvoice.last_htlc_expiry != earliest_htlc_expiry {
                        datastore_htlc_expiry(
                            &mut rpc,
                            pay_hash.to_string(),
                            earliest_htlc_expiry.to_string(),
                        )
                        .await?;
                        holdinvoice.last_htlc_expiry = earliest_htlc_expiry;
                    }
                }
            }

            if let Holdstate::Canceled = hold_state {
                info!(
                    "payment_hash: `{}`. Htlc arrived after \
                        hold-cancellation was requested. \
                        Rejecting htlc...",
                    pay_hash
                );
                let mut holdinvoices = plugin.state().holdinvoices.lock().await;
                cleanup_pluginstate_holdinvoices(&mut holdinvoices, pay_hash, &global_htlc_ident)
                    .await;

                return Ok(json!({"result": "fail",
                "failure_message": get_failure_message(
                    *plugin.state().blockheight.lock(),
                    amount_msat)
                }));
            }

            info!(
                "payment_hash: `{}` scid: `{}` htlc_id: `{}`. \
                Holding {}msat",
                pay_hash, global_htlc_ident.scid, global_htlc_ident.htlc_id, amount_msat
            );

            return loop_htlc_hold(
                plugin.clone(),
                &mut rpc,
                pay_hash,
                global_htlc_ident,
                invoice,
                cltv_expiry,
                amount_msat,
            )
            .await;
        }
    }
    warn!("htlc_accepted hook could not find htlc object");
    Ok(json!({"result": "continue"}))
}

async fn loop_htlc_hold(
    plugin: Plugin<PluginState>,
    rpc: &mut ClnRpc,
    pay_hash: &str,
    global_htlc_ident: HtlcIdentifier,
    invoice: ListinvoicesInvoices,
    cltv_expiry: u32,
    amount_msat: u64,
) -> Result<serde_json::Value, Error> {
    let mut first_iter = true;
    let cancel_hold_before_invoice_expiry_seconds =
        plugin.option(&OPT_CANCEL_HOLD_BEFORE_INVOICE_EXPIRY_SECONDS)? as u64;
    let cancel_hold_before_htlc_expiry_blocks =
        plugin.option(&OPT_CANCEL_HOLD_BEFORE_HTLC_EXPIRY_BLOCKS)? as u32;
    loop {
        if !first_iter {
            time::sleep(Duration::from_secs(2)).await;
        } else {
            first_iter = false;
        }
        {
            let mut holdinvoices = plugin.state().holdinvoices.lock().await;
            if let Some(holdinvoice_data) = holdinvoices.get_mut(&pay_hash.to_string()) {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();

                #[allow(clippy::clone_on_copy)]
                if holdinvoice_data
                    .htlc_data
                    .get(&global_htlc_ident)
                    .unwrap()
                    .loop_mutex
                    .lock()
                    .await
                    .clone()
                    || invoice.expires_at <= now + cancel_hold_before_invoice_expiry_seconds
                {
                    match listdatastore_state(rpc, pay_hash.to_string()).await {
                        Ok(s) => {
                            holdinvoice_data.hold_state = Holdstate::from_str(&s.string.unwrap())?;
                            holdinvoice_data.generation = s.generation.unwrap_or(0);
                        }
                        Err(e) => {
                            warn!(
                                "Error getting state for pay_hash: {} {}",
                                pay_hash,
                                e.to_string()
                            );
                            continue;
                        }
                    };

                    // cln cannot accept htlcs for expired invoices
                    #[allow(clippy::clone_on_copy)]
                    if cltv_expiry
                        <= plugin.state().blockheight.lock().clone()
                            + cancel_hold_before_htlc_expiry_blocks
                        || invoice.expires_at <= now + cancel_hold_before_invoice_expiry_seconds
                    {
                        match holdinvoice_data.hold_state {
                            Holdstate::Open | Holdstate::Accepted => {
                                match datastore_update_state(
                                    rpc,
                                    pay_hash.to_string(),
                                    Holdstate::Canceled.to_string(),
                                    holdinvoice_data.generation,
                                )
                                .await
                                {
                                    Ok(_o) => {
                                        warn!(
                                            "payment_hash: `{}` scid: `{}` htlc: `{}`. \
                                        holdinvoice/htlc expired! Canceling htlc...",
                                            pay_hash,
                                            global_htlc_ident.scid,
                                            global_htlc_ident.htlc_id
                                        );
                                        holdinvoice_data.hold_state = Holdstate::Canceled
                                    }
                                    Err(e) => {
                                        warn!(
                                            "Error updating state for pay_hash: {} {}",
                                            pay_hash,
                                            e.to_string()
                                        );
                                        continue;
                                    }
                                }
                            }
                            Holdstate::Canceled | Holdstate::Settled => (),
                        }
                    }

                    match holdinvoice_data.hold_state {
                        Holdstate::Open => {
                            if Amount::msat(&invoice.amount_msat.unwrap())
                                <= holdinvoice_data
                                    .htlc_data
                                    .values()
                                    .map(|htlc| htlc.amount_msat)
                                    .sum()
                                && holdinvoice_data
                                    .hold_state
                                    .is_valid_transition(&Holdstate::Accepted)
                            {
                                match datastore_update_state(
                                    rpc,
                                    pay_hash.to_string(),
                                    Holdstate::Accepted.to_string(),
                                    holdinvoice_data.generation,
                                )
                                .await
                                {
                                    Ok(_o) => (),
                                    Err(e) => {
                                        warn!(
                                            "Error updating state for pay_hash: {} {}",
                                            pay_hash,
                                            e.to_string()
                                        );
                                        continue;
                                    }
                                };
                                info!(
                                    "payment_hash: `{}` scid: `{}` htlc: `{}`. \
                                    Got enough msats for holdinvoice. \
                                    State=ACCEPTED",
                                    pay_hash, global_htlc_ident.scid, global_htlc_ident.htlc_id
                                );
                                *holdinvoice_data
                                    .htlc_data
                                    .get(&global_htlc_ident)
                                    .unwrap()
                                    .loop_mutex
                                    .lock()
                                    .await = false;
                            } else {
                                debug!(
                                    "payment_hash: `{}` scid: `{}` htlc: `{}`. \
                                    Not enough msats for holdinvoice yet.",
                                    pay_hash, global_htlc_ident.scid, global_htlc_ident.htlc_id
                                );
                            }
                        }
                        Holdstate::Accepted => {
                            if Amount::msat(&invoice.amount_msat.unwrap())
                                > holdinvoice_data
                                    .htlc_data
                                    .values()
                                    .map(|htlc| htlc.amount_msat)
                                    .sum()
                            {
                                match datastore_update_state(
                                    rpc,
                                    pay_hash.to_string(),
                                    Holdstate::Open.to_string(),
                                    holdinvoice_data.generation,
                                )
                                .await
                                {
                                    Ok(_o) => (),
                                    Err(e) => {
                                        warn!(
                                            "Error updating state for pay_hash: {} {}",
                                            pay_hash,
                                            e.to_string()
                                        );
                                        continue;
                                    }
                                };
                                warn!(
                                    "payment_hash: `{}` scid: `{}` htlc: `{}`. \
                                    No longer enough msats for holdinvoice! \
                                    This should only happen during a node restart! \
                                    Back to OPEN state!",
                                    pay_hash, global_htlc_ident.scid, global_htlc_ident.htlc_id
                                );
                            } else {
                                debug!(
                                    "payment_hash: `{}` scid: `{}` htlc: `{}`. \
                                    Holding accepted holdinvoice.",
                                    pay_hash, global_htlc_ident.scid, global_htlc_ident.htlc_id
                                );
                                *holdinvoice_data
                                    .htlc_data
                                    .get(&global_htlc_ident)
                                    .unwrap()
                                    .loop_mutex
                                    .lock()
                                    .await = false;
                            }
                        }
                        Holdstate::Settled => {
                            info!(
                                "payment_hash: `{}` scid: `{}` htlc: `{}`. \
                                    Settling htlc for holdinvoice. State=SETTLED",
                                pay_hash, global_htlc_ident.scid, global_htlc_ident.htlc_id
                            );

                            cleanup_pluginstate_holdinvoices(
                                &mut holdinvoices,
                                pay_hash,
                                &global_htlc_ident,
                            )
                            .await;

                            return Ok(json!({"result": "continue"}));
                        }
                        Holdstate::Canceled => {
                            info!(
                                "payment_hash: `{}` scid: `{}` htlc: `{}`. \
                                    Rejecting htlc for canceled holdinvoice. \
                                    State=CANCELED",
                                pay_hash, global_htlc_ident.scid, global_htlc_ident.htlc_id
                            );

                            cleanup_pluginstate_holdinvoices(
                                &mut holdinvoices,
                                pay_hash,
                                &global_htlc_ident,
                            )
                            .await;

                            return Ok(json!({"result": "fail",
                            "failure_message": get_failure_message(
                                *plugin.state().blockheight.lock(),
                                amount_msat)
                            }));
                        }
                    }
                }
            } else {
                warn!(
                    "payment_hash: `{}` scid: `{}` htlc: `{}`. \
                        DROPPED INVOICE from internal state!",
                    pay_hash, global_htlc_ident.scid, global_htlc_ident.htlc_id
                );
                return Err(anyhow!(
                    "Invoice dropped from internal state unexpectedly: {}",
                    pay_hash
                ));
            }
        }
    }
}

fn get_failure_message(blockheight: u32, amount_msat: u64) -> String {
    let hex_amount_msat = format!("{:016X}", amount_msat);
    let hex_blockheight = format!("{:08X}", blockheight);

    format!(
        "{}{}{}",
        WIRE_INCORRECT_OR_UNKNOWN_PAYMENT_DETAILS, hex_amount_msat, hex_blockheight
    )
}

pub async fn block_added(plugin: Plugin<PluginState>, v: serde_json::Value) -> Result<(), Error> {
    let block = if let Some(b) = v.get("block") {
        b
    } else if let Some(b) = v.get("block_added") {
        b
    } else {
        return Err(anyhow!("could not read block notification"));
    };
    if let Some(h) = block.get("height") {
        *plugin.state().blockheight.lock() = h.as_u64().unwrap() as u32
    } else {
        return Err(anyhow!("could not find height for block"));
    }

    let mut holdinvoices = plugin.state().holdinvoices.lock().await;
    for (_, invoice) in holdinvoices.iter_mut() {
        for (_, htlc) in invoice.htlc_data.iter_mut() {
            *htlc.loop_mutex.lock().await = true;
        }
    }

    Ok(())
}
