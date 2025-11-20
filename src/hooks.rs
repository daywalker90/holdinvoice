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
};
use serde::Deserialize;
use serde_json::json;
use tokio::time::{self};

use crate::{
    model::{HoldHtlc, HoldInvoice, HtlcIdentifier, PluginState},
    rpc::{datastore_update_state, listdatastore_state},
    util::cleanup_pluginstate_holdinvoices,
    Holdstate,
    OPT_CANCEL_HOLD_BEFORE_HTLC_EXPIRY_BLOCKS,
    OPT_CANCEL_HOLD_BEFORE_INVOICE_EXPIRY_SECONDS,
};

const WIRE_INCORRECT_OR_UNKNOWN_PAYMENT_DETAILS: &str = "400F";

#[derive(Debug, Deserialize)]
struct HtlcHook {
    htlc: Htlc,
    forward_to: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct Htlc {
    short_channel_id: ShortChannelId,
    id: u64,
    amount_msat: u64,
    cltv_expiry: u32,
    cltv_expiry_relative: u64,
    payment_hash: String,
}

pub async fn htlc_handler(
    plugin: Plugin<PluginState>,
    v: serde_json::Value,
) -> Result<serde_json::Value, Error> {
    let htlc_hook = match serde_json::from_value::<HtlcHook>(v) {
        Ok(args) => args,
        Err(err) => {
            log::warn!("htlc_accepted hook deserialization error: {err}");
            return Ok(json!({"result": "continue"}));
        }
    };
    if htlc_hook.forward_to.is_some() {
        return Ok(json!({"result": "continue"}));
    }

    log::debug!(
        "payment_hash: `{}`. htlc_hook started!",
        htlc_hook.htlc.payment_hash
    );
    // let rpc_path = make_rpc_path(plugin.clone());
    // let mut rpc = ClnRpc::new(&rpc_path)
    //     .await
    //     .context("rpc connection full")?;

    let is_new_invoice;

    let invoice;
    let global_htlc_ident;
    let hold_state;

    {
        let mut holdinvoices = plugin.state().holdinvoices.lock().await;
        let generation;
        if let Some(holdinvoice) = holdinvoices.get_mut(&htlc_hook.htlc.payment_hash) {
            is_new_invoice = false;
            log::debug!(
                "payment_hash: `{}`. Htlc is for a known holdinvoice! Processing...",
                htlc_hook.htlc.payment_hash
            );

            hold_state = holdinvoice.hold_state;
            invoice = holdinvoice.invoice.clone();
            generation = holdinvoice.generation;
        } else {
            is_new_invoice = true;
            log::debug!(
                "payment_hash: `{}`. New htlc, checking if it's our invoice...",
                htlc_hook.htlc.payment_hash
            );
            let mut rpc = plugin.state().rpc.lock().await;

            match listdatastore_state(&mut rpc, htlc_hook.htlc.payment_hash.clone()).await {
                Ok(dbstate) => {
                    log::debug!(
                        "payment_hash: `{}`. Htlc is for a holdinvoice! Processing...",
                        htlc_hook.htlc.payment_hash
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
                            payment_hash: Some(htlc_hook.htlc.payment_hash.clone()),
                            start: None,
                        })
                        .await?
                        .invoices
                        .first()
                        .ok_or(anyhow!(
                            "payment_hash: `{}`. holdinvoice not found!",
                            htlc_hook.htlc.payment_hash
                        ))?
                        .clone();
                }
                Err(_e) => {
                    log::debug!(
                        "payment_hash: `{}`. Not a holdinvoice! Continue...",
                        htlc_hook.htlc.payment_hash
                    );
                    return Ok(json!({"result": "continue"}));
                }
            };
        }

        global_htlc_ident = HtlcIdentifier {
            htlc_id: htlc_hook.htlc.id,
            scid: htlc_hook.htlc.short_channel_id,
        };

        if is_new_invoice {
            let mut htlc_data = HashMap::new();
            htlc_data.insert(
                global_htlc_ident,
                HoldHtlc {
                    amount_msat: htlc_hook.htlc.amount_msat,
                    cltv_expiry: htlc_hook.htlc.cltv_expiry,
                    loop_mutex: Arc::new(tokio::sync::Mutex::new(true)),
                },
            );
            holdinvoices.insert(
                htlc_hook.htlc.payment_hash.clone(),
                HoldInvoice {
                    hold_state,
                    generation,
                    htlc_data,
                    invoice: invoice.clone(),
                },
            );
        } else {
            let holdinvoice = holdinvoices.get_mut(&htlc_hook.htlc.payment_hash).unwrap();
            holdinvoice.htlc_data.insert(
                global_htlc_ident,
                HoldHtlc {
                    amount_msat: htlc_hook.htlc.amount_msat,
                    cltv_expiry: htlc_hook.htlc.cltv_expiry,
                    loop_mutex: Arc::new(tokio::sync::Mutex::new(true)),
                },
            );
        }
    }

    if let Holdstate::Canceled = hold_state {
        log::info!(
            "payment_hash: `{}`. Htlc arrived after \
                        hold-cancellation was requested. \
                        Rejecting htlc...",
            htlc_hook.htlc.payment_hash
        );
        let mut holdinvoices = plugin.state().holdinvoices.lock().await;
        cleanup_pluginstate_holdinvoices(
            &mut holdinvoices,
            &htlc_hook.htlc.payment_hash,
            &global_htlc_ident,
        )
        .await;

        return Ok(json!({"result": "fail",
        "failure_message": get_failure_message(
            *plugin.state().blockheight.lock(),
            htlc_hook.htlc.amount_msat)
        }));
    }

    log::info!(
        "payment_hash: `{}` scid: `{}` htlc_id: `{}`. \
                Holding {}msat",
        htlc_hook.htlc.payment_hash,
        global_htlc_ident.scid,
        global_htlc_ident.htlc_id,
        htlc_hook.htlc.amount_msat
    );

    return loop_htlc_hold(
        plugin.clone(),
        &htlc_hook.htlc.payment_hash,
        global_htlc_ident,
        invoice,
        htlc_hook.htlc.cltv_expiry,
        htlc_hook.htlc.amount_msat,
    )
    .await;
}

async fn loop_htlc_hold(
    plugin: Plugin<PluginState>,
    payment_hash: &str,
    global_htlc_ident: HtlcIdentifier,
    invoice: ListinvoicesInvoices,
    cltv_expiry: u32,
    amount_msat: u64,
) -> Result<serde_json::Value, Error> {
    let mut first_iter = true;
    let cancel_hold_before_invoice_expiry_seconds =
        u64::try_from(plugin.option(&OPT_CANCEL_HOLD_BEFORE_INVOICE_EXPIRY_SECONDS)?).unwrap();
    let cancel_hold_before_htlc_expiry_blocks =
        u32::try_from(plugin.option(&OPT_CANCEL_HOLD_BEFORE_HTLC_EXPIRY_BLOCKS)?).unwrap();
    loop {
        if first_iter {
            first_iter = false;
        } else {
            time::sleep(Duration::from_secs(2)).await;
        }

        let mut holdinvoices = plugin.state().holdinvoices.lock().await;
        let Some(holdinvoice_data) = holdinvoices.get_mut(payment_hash) else {
            log::warn!(
                "payment_hash: `{}` scid: `{}` htlc: `{}`. \
DROPPED INVOICE from internal state!",
                payment_hash,
                global_htlc_ident.scid,
                global_htlc_ident.htlc_id
            );
            return Err(anyhow!(
                "Invoice dropped from internal state unexpectedly: {payment_hash}"
            ));
        };
        let mut rpc = plugin.state().rpc.lock().await;
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
            match listdatastore_state(&mut rpc, payment_hash.to_owned()).await {
                Ok(s) => {
                    holdinvoice_data.hold_state = Holdstate::from_str(&s.string.unwrap())?;
                    holdinvoice_data.generation = s.generation.unwrap_or(0);
                }
                Err(e) => {
                    log::warn!("Error getting state for payment_hash: {payment_hash} {e}");
                    continue;
                }
            }

            // cln cannot accept htlcs for expired invoices
            #[allow(clippy::clone_on_copy)]
            let blockheight = plugin.state().blockheight.lock().clone();
            let soft_expired = cltv_expiry <= blockheight + cancel_hold_before_htlc_expiry_blocks
                || invoice.expires_at <= now + cancel_hold_before_invoice_expiry_seconds;
            let hard_expired = cltv_expiry <= blockheight || invoice.expires_at <= now;
            if soft_expired && holdinvoice_data.hold_state == Holdstate::Accepted && !hard_expired {
                match datastore_update_state(
                    &mut rpc,
                    payment_hash.to_owned(),
                    Holdstate::Settled.to_string(),
                    holdinvoice_data.generation,
                )
                .await
                {
                    Ok(_o) => {
                        log::info!(
                            "payment_hash: `{}` scid: `{}` htlc: `{}`. \
                            holdinvoice/htlc about to expire! Settling htlc...",
                            payment_hash,
                            global_htlc_ident.scid,
                            global_htlc_ident.htlc_id
                        );
                        holdinvoice_data.hold_state = Holdstate::Settled;
                    }
                    Err(e) => {
                        log::warn!("Error updating state for payment_hash: {payment_hash} {e}");
                        continue;
                    }
                }
            } else if (soft_expired && holdinvoice_data.hold_state == Holdstate::Open)
                || hard_expired
            {
                match datastore_update_state(
                    &mut rpc,
                    payment_hash.to_owned(),
                    Holdstate::Canceled.to_string(),
                    holdinvoice_data.generation,
                )
                .await
                {
                    Ok(_o) => {
                        log::warn!(
                            "payment_hash: `{}` scid: `{}` htlc: `{}`. \
                            holdinvoice/htlc expired! Canceling htlc...",
                            payment_hash,
                            global_htlc_ident.scid,
                            global_htlc_ident.htlc_id
                        );
                        holdinvoice_data.hold_state = Holdstate::Canceled;
                    }
                    Err(e) => {
                        log::warn!("Error updating state for payment_hash: {payment_hash} {e}");
                        continue;
                    }
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
                            .is_valid_transition(Holdstate::Accepted)
                    {
                        match datastore_update_state(
                            &mut rpc,
                            payment_hash.to_owned(),
                            Holdstate::Accepted.to_string(),
                            holdinvoice_data.generation,
                        )
                        .await
                        {
                            Ok(_o) => (),
                            Err(e) => {
                                log::warn!(
                                    "Error updating state for payment_hash: {payment_hash} {e}"
                                );
                                continue;
                            }
                        }
                        log::info!(
                            "payment_hash: `{}` scid: `{}` htlc: `{}`. \
                                    Got enough msats for holdinvoice. \
                                    State=ACCEPTED",
                            payment_hash,
                            global_htlc_ident.scid,
                            global_htlc_ident.htlc_id
                        );
                        *holdinvoice_data
                            .htlc_data
                            .get(&global_htlc_ident)
                            .unwrap()
                            .loop_mutex
                            .lock()
                            .await = false;
                    } else {
                        log::debug!(
                            "payment_hash: `{}` scid: `{}` htlc: `{}`. \
                                    Not enough msats for holdinvoice yet.",
                            payment_hash,
                            global_htlc_ident.scid,
                            global_htlc_ident.htlc_id
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
                            &mut rpc,
                            payment_hash.to_owned(),
                            Holdstate::Open.to_string(),
                            holdinvoice_data.generation,
                        )
                        .await
                        {
                            Ok(_o) => (),
                            Err(e) => {
                                log::warn!(
                                    "Error updating state for payment_hash: {payment_hash} {e}"
                                );
                                continue;
                            }
                        }
                        log::warn!(
                            "payment_hash: `{}` scid: `{}` htlc: `{}`. \
                                    No longer enough msats for holdinvoice! \
                                    This should only happen during a node restart! \
                                    Back to OPEN state!",
                            payment_hash,
                            global_htlc_ident.scid,
                            global_htlc_ident.htlc_id
                        );
                    } else {
                        log::debug!(
                            "payment_hash: `{}` scid: `{}` htlc: `{}`. \
                                    Holding accepted holdinvoice.",
                            payment_hash,
                            global_htlc_ident.scid,
                            global_htlc_ident.htlc_id
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
                    log::info!(
                        "payment_hash: `{}` scid: `{}` htlc: `{}`. \
                                    Settling htlc for holdinvoice. State=SETTLED",
                        payment_hash,
                        global_htlc_ident.scid,
                        global_htlc_ident.htlc_id
                    );

                    cleanup_pluginstate_holdinvoices(
                        &mut holdinvoices,
                        payment_hash,
                        &global_htlc_ident,
                    )
                    .await;

                    return Ok(json!({"result": "continue"}));
                }
                Holdstate::Canceled => {
                    log::info!(
                        "payment_hash: `{}` scid: `{}` htlc: `{}`. \
                                    Rejecting htlc for canceled holdinvoice. \
                                    State=CANCELED",
                        payment_hash,
                        global_htlc_ident.scid,
                        global_htlc_ident.htlc_id
                    );

                    cleanup_pluginstate_holdinvoices(
                        &mut holdinvoices,
                        payment_hash,
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
    }
}

fn get_failure_message(blockheight: u32, amount_msat: u64) -> String {
    let hex_amount_msat = format!("{amount_msat:016X}");
    let hex_blockheight = format!("{blockheight:08X}");

    format!("{WIRE_INCORRECT_OR_UNKNOWN_PAYMENT_DETAILS}{hex_amount_msat}{hex_blockheight}")
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
        *plugin.state().blockheight.lock() = u32::try_from(h.as_u64().unwrap())?;
    } else {
        return Err(anyhow!("could not find height for block"));
    }

    let mut holdinvoices = plugin.state().holdinvoices.lock().await;
    for (_, invoice) in holdinvoices.iter_mut() {
        for htlc in invoice.htlc_data.values_mut() {
            *htlc.loop_mutex.lock().await = true;
        }
    }

    Ok(())
}
