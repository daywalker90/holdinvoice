use std::{str::FromStr, time::Duration};

use anyhow::{anyhow, Error};
use cln_plugin::Plugin;
use cln_rpc::{
    model::responses::{ListinvoicesInvoicesStatus, ListpeerchannelsChannelsState},
    ClnRpc, Request, Response,
};
use log::{debug, warn};
use serde_json::json;
use tokio::{time, time::Instant};

use crate::{
    errors::*,
    model::{HoldLookupResponse, HoldStateResponse, PluginState},
    rpc::{
        datastore_new_state, datastore_update_state_forced, listdatastore_htlc_expiry,
        listdatastore_state, listinvoices, listpeerchannels,
    },
    util::{build_invoice_request, make_rpc_path, parse_payment_hash},
    Holdstate,
};

pub async fn hold_invoice(
    plugin: Plugin<PluginState>,
    args: serde_json::Value,
) -> Result<serde_json::Value, Error> {
    let rpc_path = make_rpc_path(plugin.clone());
    let mut rpc = ClnRpc::new(&rpc_path).await?;

    let valid_arg_keys = [
        "amount_msat",
        "label",
        "description",
        "expiry",
        "fallbacks",
        "preimage",
        "cltv",
        "deschashonly",
    ];

    let mut new_args = serde_json::Value::Object(Default::default());
    match args {
        serde_json::Value::Array(a) => {
            for (idx, arg) in a.iter().enumerate() {
                if idx < valid_arg_keys.len() {
                    new_args[valid_arg_keys[idx]] = arg.clone();
                }
            }
        }
        serde_json::Value::Object(o) => {
            for (k, v) in o.iter() {
                if !valid_arg_keys.contains(&k.as_str()) {
                    return Ok(invalid_argument_error(k));
                }
                new_args[k] = v.clone();
            }
        }
        _ => return Ok(invalid_input_error(&args.to_string())),
    };

    let inv_req = match build_invoice_request(&new_args, &plugin) {
        Ok(i) => i,
        Err(e) => return Ok(e),
    };

    let invoice_request = match rpc.call(Request::Invoice(inv_req)).await {
        Ok(resp) => resp,
        Err(e) => match e.code {
            Some(_) => return Ok(json!(e)),
            None => return Err(anyhow!("Unexpected response in invoice: {}", e.to_string())),
        },
    };
    let result = match invoice_request {
        Response::Invoice(info) => info,
        e => return Err(anyhow!("Unexpected result in invoice: {:?}", e)),
    };
    datastore_new_state(
        &rpc_path,
        result.payment_hash.to_string(),
        Holdstate::Open.to_string(),
    )
    .await?;
    Ok(json!(result))
}

pub async fn hold_invoice_settle(
    plugin: Plugin<PluginState>,
    args: serde_json::Value,
) -> Result<serde_json::Value, Error> {
    let rpc_path = make_rpc_path(plugin.clone());

    let pay_hash = match parse_payment_hash(args) {
        Ok(ph) => ph,
        Err(e) => return Ok(e),
    };

    let data = match listdatastore_state(&rpc_path, pay_hash.clone()).await {
        Ok(d) => d,
        Err(_) => return Ok(payment_hash_missing_error(&pay_hash)),
    };

    let holdstate = Holdstate::from_str(&data.string.unwrap())?;

    if holdstate.is_valid_transition(&Holdstate::Settled) {
        let result = datastore_update_state_forced(
            &rpc_path,
            pay_hash.clone(),
            Holdstate::Settled.to_string(),
        )
        .await;
        match result {
            Ok(_r) => {
                let mut holdinvoices = plugin.state().holdinvoices.lock().await;
                if let Some(invoice) = holdinvoices.get_mut(&pay_hash.to_string()) {
                    for (_, htlc) in invoice.htlc_data.iter_mut() {
                        *htlc.loop_mutex.lock().await = true;
                    }
                } else {
                    warn!(
                        "payment_hash: '{}' DROPPED INVOICE from internal state!",
                        pay_hash
                    );
                    return Err(anyhow!(
                        "Invoice dropped from internal state unexpectedly: {}",
                        pay_hash
                    ));
                }

                Ok(json!(HoldStateResponse {
                    state: Holdstate::Settled.to_string(),
                }))
            }
            Err(e) => {
                debug!(
                    "Unexpected result {} to method call datastore_update_state_forced",
                    e.to_string()
                );
                Err(anyhow!(
                    "Unexpected result {} to method call datastore_update_state_forced",
                    e.to_string()
                ))
            }
        }
    } else {
        Ok(wrong_hold_state_error(holdstate))
    }
}

pub async fn hold_invoice_cancel(
    plugin: Plugin<PluginState>,
    args: serde_json::Value,
) -> Result<serde_json::Value, Error> {
    let rpc_path = make_rpc_path(plugin.clone());

    let pay_hash = match parse_payment_hash(args) {
        Ok(ph) => ph,
        Err(e) => return Ok(e),
    };

    let data = match listdatastore_state(&rpc_path, pay_hash.clone()).await {
        Ok(d) => d,
        Err(_) => return Ok(payment_hash_missing_error(&pay_hash)),
    };

    let holdstate = Holdstate::from_str(&data.string.unwrap())?;

    if holdstate.is_valid_transition(&Holdstate::Canceled) {
        let result = datastore_update_state_forced(
            &rpc_path,
            pay_hash.clone(),
            Holdstate::Canceled.to_string(),
        )
        .await;
        match result {
            Ok(_r) => {
                let mut holdinvoices = plugin.state().holdinvoices.lock().await;
                if let Some(invoice) = holdinvoices.get_mut(&pay_hash) {
                    for (_, htlc) in invoice.htlc_data.iter_mut() {
                        *htlc.loop_mutex.lock().await = true;
                    }
                }

                Ok(json!(HoldStateResponse {
                    state: Holdstate::Canceled.to_string(),
                }))
            }
            Err(e) => Err(anyhow!(
                "Unexpected result {} to method call datastore_update_state_forced",
                e.to_string()
            )),
        }
    } else {
        Ok(wrong_hold_state_error(holdstate))
    }
}

pub async fn hold_invoice_lookup(
    plugin: Plugin<PluginState>,
    args: serde_json::Value,
) -> Result<serde_json::Value, Error> {
    let rpc_path = make_rpc_path(plugin.clone());

    let pay_hash = match parse_payment_hash(args) {
        Ok(ph) => ph,
        Err(e) => return Ok(e),
    };

    let data = match listdatastore_state(&rpc_path, pay_hash.clone()).await {
        Ok(d) => d,
        Err(_) => return Ok(payment_hash_missing_error(&pay_hash)),
    };

    let holdstate = Holdstate::from_str(&data.string.unwrap())?;

    let mut htlc_expiry = None;
    match holdstate {
        Holdstate::Open => {
            let invoices = listinvoices(&rpc_path, None, Some(pay_hash.clone()))
                .await?
                .invoices;
            if let Some(inv) = invoices.first() {
                if inv.status == ListinvoicesInvoicesStatus::EXPIRED {
                    datastore_update_state_forced(
                        &rpc_path,
                        pay_hash.clone(),
                        Holdstate::Canceled.to_string(),
                    )
                    .await?;
                    return Ok(json!(HoldLookupResponse {
                        state: Holdstate::Canceled.to_string(),
                        htlc_expiry
                    }));
                }
            } else {
                return Ok(payment_hash_missing_error(&pay_hash));
            }
        }
        Holdstate::Accepted => {
            htlc_expiry = Some(listdatastore_htlc_expiry(&rpc_path, pay_hash.clone()).await?)
        }
        Holdstate::Canceled => {
            let now = Instant::now();
            loop {
                let mut all_cancelled = true;
                let channels = match listpeerchannels(&rpc_path).await?.channels {
                    Some(c) => c,
                    None => break,
                };

                for chan in channels {
                    let connected = if let Some(c) = chan.peer_connected {
                        c
                    } else {
                        continue;
                    };
                    let state = if let Some(s) = chan.state {
                        s
                    } else {
                        continue;
                    };
                    if !connected
                        || state != ListpeerchannelsChannelsState::CHANNELD_NORMAL
                            && state != ListpeerchannelsChannelsState::CHANNELD_AWAITING_SPLICE
                    {
                        continue;
                    }

                    let htlcs = if let Some(h) = chan.htlcs {
                        h
                    } else {
                        continue;
                    };
                    for htlc in htlcs {
                        if let Some(ph) = htlc.payment_hash {
                            if ph.to_string() == pay_hash {
                                all_cancelled = false;
                            }
                        }
                    }
                }

                if all_cancelled {
                    break;
                }

                if now.elapsed().as_secs() > 20 {
                    return Err(anyhow!(
                        "holdinvoicelookup: Timed out before cancellation of all \
                        related htlcs was finished"
                    ));
                }

                time::sleep(Duration::from_secs(2)).await
            }
        }
        Holdstate::Settled => {
            let now = Instant::now();
            loop {
                let invoices = listinvoices(&rpc_path, None, Some(pay_hash.clone()))
                    .await?
                    .invoices;

                if let Some(inv) = invoices.first() {
                    match inv.status {
                        ListinvoicesInvoicesStatus::PAID => {
                            break;
                        }
                        ListinvoicesInvoicesStatus::EXPIRED => {
                            return Err(anyhow!(
                                "holdinvoicelookup: Invoice expired while trying to settle!"
                            ));
                        }
                        _ => (),
                    }
                }

                if now.elapsed().as_secs() > 20 {
                    return Err(anyhow!(
                        "holdinvoicelookup: Timed out before settlement could be confirmed",
                    ));
                }

                time::sleep(Duration::from_secs(2)).await
            }
        }
    }
    Ok(json!(HoldLookupResponse {
        state: holdstate.to_string(),
        htlc_expiry
    }))
}
