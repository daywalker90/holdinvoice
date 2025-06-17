use std::{
    str::FromStr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, Error};
use bitcoin::hashes::{sha256, Hash};
use cln_plugin::Plugin;
use cln_rpc::{
    model::requests::{DecodeRequest, ListpeerchannelsRequest},
    primitives::ChannelState,
};
use lightning_invoice::Bolt11Invoice;
use serde_json::json;
use tokio::time::{self, Instant};

use crate::{
    errors::*,
    model::{HoldInvoiceResponse, HoldLookupResponse, HoldStateResponse, PluginState},
    rpc::{
        datastore_new_hold_invoice, datastore_update_hold_invoice_forced,
        datastore_update_state_forced, listdatastore_all, listdatastore_payment_hash,
    },
    util::{build_invoice_request, parse_payment_hash, parse_preimage},
    Holdstate,
};

pub async fn hold_invoice(
    plugin: Plugin<PluginState>,
    args: serde_json::Value,
) -> Result<serde_json::Value, Error> {
    let mut rpc = plugin.state().method_rpc.lock().await;

    let valid_arg_keys = [
        "amount_msat",
        "description",
        "expiry",
        "payment_hash",
        "preimage",
        "cltv",
        "deschashonly",
        "exposeprivatechannels",
    ];

    let mut new_args = serde_json::Value::Object(Default::default());
    match args {
        serde_json::Value::Array(a) => {
            if a.len() > valid_arg_keys.len() {
                return Err(too_many_params_error(a.len(), valid_arg_keys.len()));
            }
            for (idx, arg) in a.iter().enumerate() {
                new_args[valid_arg_keys[idx]] = arg.clone();
            }
        }
        serde_json::Value::Object(o) => {
            for (k, v) in o.iter() {
                if !valid_arg_keys.contains(&k.as_str()) {
                    return Err(invalid_argument_error(k));
                }
                new_args[k] = v.clone();
            }
        }
        _ => return Err(invalid_input_error(&args.to_string())),
    };

    let (invoice, preimage, description) =
        match build_invoice_request(&new_args, &plugin, &mut rpc).await {
            Ok(o) => o,
            Err(e) => {
                log::warn!("Error building invoice: {}", e);
                return Err(e);
            }
        };

    let decoded_invoice = match rpc
        .call_typed(&DecodeRequest {
            string: invoice.clone(),
        })
        .await
    {
        Ok(d) => d,
        Err(e) => return Err(internal_error(&e.to_string())),
    };

    if !decoded_invoice.valid {
        return Err(anyhow!("invalid invoice"));
    }
    let payment_hash: String = match decoded_invoice.item_type {
        cln_rpc::model::responses::DecodeType::BOLT11_INVOICE => hex::encode(
            decoded_invoice
                .payment_hash
                .ok_or_else(|| anyhow!("payment_hash not found in decoded invoice"))?,
        ),
        _ => return Err(anyhow!("not a bolt11 invoice")),
    };

    let payment_secret = if let Some(ps) = decoded_invoice.payment_secret {
        hex::encode(&ps.to_vec())
    } else {
        return Err(anyhow!("payment_secret not found in decoded invoice"));
    };

    let mut response = HoldInvoiceResponse {
        bolt11: invoice,
        payment_hash: payment_hash.clone(),
        payment_secret,
        expires_at: decoded_invoice.created_at.unwrap() + decoded_invoice.expiry.unwrap(),
        preimage,
        description: description.clone(),
        description_hash: None,
        state: Holdstate::Open,
        htlc_expiry: None,
        paid_at: None,
    };

    datastore_new_hold_invoice(&mut rpc, payment_hash.clone(), response.clone()).await?;

    response.description_hash = if description.is_some() {
        response.description = None;
        Some(hex::encode(decoded_invoice.description_hash.ok_or_else(
            || anyhow!("description_hash not found in decoded invoice"),
        )?))
    } else {
        response.description = Some(
            decoded_invoice
                .description
                .ok_or_else(|| anyhow!("description not found in decoded invoice"))?,
        );
        None
    };

    log::trace!("HoldInvoiceResponse: {:?}", response);
    Ok(json!(response))
}

pub async fn hold_invoice_settle(
    plugin: Plugin<PluginState>,
    args: serde_json::Value,
) -> Result<serde_json::Value, Error> {
    let mut rpc = plugin.state().method_rpc.lock().await;

    let payment_hash_str = parse_payment_hash(&args).ok();

    let preimage_str = parse_preimage(&args).ok();

    if payment_hash_str.is_none() && preimage_str.is_none() {
        return Err(invalid_input_error(
            "neither payment_hash nor preimage were valid, make sure to use `-k` on the CLI",
        ));
    }

    if payment_hash_str.is_some() && preimage_str.is_some() {
        return Err(invalid_input_error(
            "only provide one of payment_hash or preimage",
        ));
    }

    let payment_hash_str = if let Some(ph) = payment_hash_str {
        ph
    } else {
        let payment_hash_from_preimage: sha256::Hash =
            Hash::hash(&hex::decode(preimage_str.as_ref().unwrap())?);
        log::debug!(
            "Generated payment_hash: {} from preimage: {}",
            payment_hash_from_preimage,
            preimage_str.as_ref().unwrap()
        );
        payment_hash_from_preimage.to_string()
    };

    let (mut holdinvoice, _generation) =
        listdatastore_payment_hash(&mut rpc, &payment_hash_str).await?;

    if let Some(pi) = preimage_str {
        holdinvoice.preimage = Some(pi);
    }

    if holdinvoice.preimage.is_none() {
        return Err(invalid_input_error(
            "Must provide missing preimage instead of payment_hash!",
        ));
    };

    if holdinvoice.state.is_valid_new_state(&Holdstate::Settled) {
        holdinvoice.paid_at = Some(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs());
        holdinvoice.state = Holdstate::Settled;
        let result = datastore_update_hold_invoice_forced(
            &mut rpc,
            payment_hash_str.clone(),
            holdinvoice.clone(),
        )
        .await;
        match result {
            Ok(_r) => {
                {
                    let mut holdinvoices = plugin.state().holdinvoices.lock().await;
                    if let Some(invoice) = holdinvoices.get_mut(&payment_hash_str) {
                        for (_, htlc) in invoice.htlc_data.iter_mut() {
                            *htlc.loop_mutex.lock().await = true;
                        }
                    } else {
                        log::warn!(
                            "payment_hash: '{}' DROPPED INVOICE from internal state!",
                            payment_hash_str
                        );
                        return Err(anyhow!(
                            "Invoice dropped from internal state unexpectedly: {}",
                            payment_hash_str
                        ));
                    }
                }

                let now = Instant::now();

                loop {
                    let mut all_settled = true;
                    let channels = rpc
                        .call_typed(&ListpeerchannelsRequest { id: None })
                        .await?
                        .channels;

                    for chan in channels {
                        if chan.state != ChannelState::CHANNELD_NORMAL
                            && chan.state != ChannelState::CHANNELD_AWAITING_SPLICE
                        {
                            log::trace!(
                                "skipping non-normal channel {:?} connected:{} state: {:?}",
                                chan.short_channel_id,
                                chan.peer_connected,
                                chan.state
                            );
                            continue;
                        }

                        let htlcs = if let Some(h) = chan.htlcs {
                            h
                        } else {
                            log::trace!("{:?} has no htlc object", chan.short_channel_id);
                            continue;
                        };
                        for htlc in htlcs {
                            log::trace!(
                                "is htlc ours? htlc:{} inv:{}",
                                htlc.payment_hash,
                                holdinvoice.payment_hash
                            );
                            if htlc
                                .payment_hash
                                .to_string()
                                .eq_ignore_ascii_case(&holdinvoice.payment_hash)
                            {
                                all_settled = false;
                            }
                        }
                    }

                    if all_settled {
                        break;
                    }

                    if now.elapsed().as_secs() > 30 {
                        return Err(anyhow!(
                            "holdinvoicelookup: Timed out before settlement could be confirmed",
                        ));
                    }

                    time::sleep(Duration::from_secs(2)).await
                }

                Ok(json!(HoldStateResponse {
                    state: Holdstate::Settled,
                }))
            }
            Err(e) => {
                log::debug!(
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
        Err(wrong_hold_state_error(holdinvoice.state))
    }
}

pub async fn hold_invoice_cancel(
    plugin: Plugin<PluginState>,
    args: serde_json::Value,
) -> Result<serde_json::Value, Error> {
    let mut rpc = plugin.state().method_rpc.lock().await;

    let args = if let serde_json::Value::Array(a) = args {
        if a.is_empty() {
            return Err(missing_parameter_error("payment_hash"));
        } else if a.len() == 1 {
            json!({
                "payment_hash": a[0]
            })
        } else {
            return Err(too_many_params_error(a.len(), 1));
        }
    } else {
        args
    };

    let pay_hash = parse_payment_hash(&args)?;

    let (holdinvoice, _generation) = match listdatastore_payment_hash(&mut rpc, &pay_hash).await {
        Ok(d) => d,
        Err(_) => return Err(payment_hash_missing_error(&pay_hash)),
    };

    if holdinvoice.state.is_valid_new_state(&Holdstate::Canceled) {
        let result =
            datastore_update_state_forced(&mut rpc, pay_hash.clone(), Holdstate::Canceled).await;
        match result {
            Ok(_r) => {
                {
                    let mut holdinvoices = plugin.state().holdinvoices.lock().await;
                    if let Some(invoice) = holdinvoices.get_mut(&pay_hash) {
                        for (_, htlc) in invoice.htlc_data.iter_mut() {
                            *htlc.loop_mutex.lock().await = true;
                        }
                    }
                }

                let now = Instant::now();
                loop {
                    let mut all_cancelled = true;
                    let channels = rpc
                        .call_typed(&ListpeerchannelsRequest { id: None })
                        .await?
                        .channels;
                    log::trace!("channels: {}", channels.len());

                    for chan in channels {
                        if chan.state != ChannelState::CHANNELD_NORMAL
                            && chan.state != ChannelState::CHANNELD_AWAITING_SPLICE
                        {
                            log::trace!(
                                "skipping non-normal channel {:?} connected:{} state: {:?}",
                                chan.short_channel_id,
                                chan.peer_connected,
                                chan.state
                            );
                            continue;
                        }

                        let htlcs = if let Some(h) = chan.htlcs {
                            h
                        } else {
                            log::trace!("{:?} has no htlc object", chan.short_channel_id);
                            continue;
                        };
                        for htlc in htlcs {
                            log::trace!(
                                "is htlc ours? htlc:{} inv:{}",
                                htlc.payment_hash,
                                holdinvoice.payment_hash
                            );
                            if htlc
                                .payment_hash
                                .to_string()
                                .eq_ignore_ascii_case(&holdinvoice.payment_hash)
                            {
                                all_cancelled = false;
                            }
                        }
                    }

                    if all_cancelled {
                        break;
                    }

                    if now.elapsed().as_secs() > 30 {
                        return Err(anyhow!(
                            "holdinvoicelookup: Timed out before cancellation of all \
                        related htlcs was finished"
                        ));
                    }

                    time::sleep(Duration::from_secs(2)).await
                }

                Ok(json!(HoldStateResponse {
                    state: Holdstate::Canceled,
                }))
            }
            Err(e) => Err(anyhow!(
                "Unexpected result {} to method call datastore_update_state_forced",
                e.to_string()
            )),
        }
    } else {
        Err(wrong_hold_state_error(holdinvoice.state))
    }
}

pub async fn hold_invoice_lookup(
    plugin: Plugin<PluginState>,
    args: serde_json::Value,
) -> Result<serde_json::Value, Error> {
    let mut rpc = plugin.state().method_rpc.lock().await;

    let payment_hash_input = if let serde_json::Value::Array(a) = args {
        if a.is_empty() {
            return Err(missing_parameter_error("payment_hash"));
        } else if a.len() == 1 {
            Some(parse_payment_hash(&json!({
                "payment_hash": a[0]
            }))?)
        } else {
            return Err(too_many_params_error(a.len(), 1));
        }
    } else if let serde_json::Value::Object(ref o) = args {
        if o.get("payment_hash").is_some() {
            Some(parse_payment_hash(&args)?)
        } else {
            None
        }
    } else {
        return Err(invalid_input_error(&args.to_string()));
    };

    let mut holdinvoices_db = if let Some(ph) = payment_hash_input {
        let (h, _g) = listdatastore_payment_hash(&mut rpc, &ph).await?;
        vec![h]
    } else {
        listdatastore_all(&mut rpc).await?
    };

    for holdinvoice in holdinvoices_db.iter_mut() {
        match holdinvoice.state {
            Holdstate::Open => {
                let invoice = Bolt11Invoice::from_str(&holdinvoice.bolt11)?;

                if invoice.is_expired() {
                    datastore_update_state_forced(
                        &mut rpc,
                        holdinvoice.payment_hash.clone(),
                        Holdstate::Canceled,
                    )
                    .await?;
                    holdinvoice.state = Holdstate::Canceled;
                }
            }
            Holdstate::Accepted => {
                let holdinvoices_internal = plugin.state().holdinvoices.lock().await;
                let next_expiry =
                    if let Some(h) = holdinvoices_internal.get(&holdinvoice.payment_hash) {
                        h.htlc_data
                            .values()
                            .map(|htlc| htlc.cltv_expiry)
                            .min()
                            .unwrap()
                    } else {
                        return Err(payment_hash_missing_error(&holdinvoice.payment_hash));
                    };
                holdinvoice.htlc_expiry = Some(next_expiry)
            }
            Holdstate::Canceled => {}
            Holdstate::Settled => {}
        }
    }

    Ok(json!(HoldLookupResponse {
        holdinvoices: holdinvoices_db
    }))
}

pub async fn holdinvoice_version(
    _p: Plugin<PluginState>,
    _args: serde_json::Value,
) -> Result<serde_json::Value, anyhow::Error> {
    Ok(json!({ "version": format!("v{}",env!("CARGO_PKG_VERSION")) }))
}
