use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use crate::model::{PluginState, DEFAULT_CLTV_DELTA, DEFAULT_EXPIRY};
use crate::{errors::*, OPT_CANCEL_HOLD_BEFORE_HTLC_EXPIRY_BLOCKS};

use anyhow::anyhow;
use bitcoin::hashes::{sha256, Hash};
use bitcoin::key::rand::{self, RngCore};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::secp256k1::{PublicKey, SecretKey};
use cln_plugin::Plugin;
use cln_rpc::model::requests::{
    DecodeRequest, DelinvoiceRequest, DelinvoiceStatus, InvoiceRequest, ListpeerchannelsRequest,
    SigninvoiceRequest,
};
use cln_rpc::primitives::{ChannelState, ShortChannelId};
use cln_rpc::ClnRpc;
use lightning_invoice::{InvoiceBuilder, PaymentSecret, RouteHint, RouteHintHop, RoutingFees};

use crate::model::{HoldInvoice, HtlcIdentifier};

pub fn make_rpc_path(plugin: Plugin<PluginState>) -> PathBuf {
    Path::new(&plugin.configuration().lightning_dir).join(plugin.configuration().rpc_file)
}

pub async fn cleanup_pluginstate_holdinvoices(
    hold_invoices: &mut BTreeMap<String, HoldInvoice>,
    pay_hash: &str,
    global_htlc_ident: &HtlcIdentifier,
) {
    if let Some(h_inv) = hold_invoices.get_mut(pay_hash) {
        h_inv.htlc_data.remove(global_htlc_ident);
        if h_inv.htlc_data.is_empty() {
            hold_invoices.remove(pay_hash);
        }
    }
}

pub fn parse_payment_hash(args: &serde_json::Value) -> Result<String, anyhow::Error> {
    if let serde_json::Value::Object(o) = args {
        let valid_arg_keys = ["payment_hash"];
        for (k, _v) in o.iter() {
            if !valid_arg_keys.contains(&k.as_str()) {
                return Err(invalid_argument_error(k));
            }
        }
        if let Some(pay_hash) = o.get("payment_hash") {
            if let serde_json::Value::String(s) = pay_hash {
                if s.len() != 64 {
                    Err(invalid_hash_error("payment_hash", s))
                } else {
                    Ok(s.clone())
                }
            } else {
                Err(invalid_hash_error("payment_hash", &pay_hash.to_string()))
            }
        } else {
            Err(missing_parameter_error("payment_hash"))
        }
    } else {
        Err(invalid_input_error(&args.to_string()))
    }
}

pub fn parse_preimage(args: &serde_json::Value) -> Result<String, anyhow::Error> {
    if let serde_json::Value::Object(o) = args {
        let valid_arg_keys = ["preimage"];
        for (k, _v) in o.iter() {
            if !valid_arg_keys.contains(&k.as_str()) {
                return Err(invalid_argument_error(k));
            }
        }
        if let Some(preimage) = o.get("preimage") {
            if let serde_json::Value::String(s) = preimage {
                if s.len() != 64 {
                    Err(invalid_hash_error("preimage", s))
                } else {
                    Ok(s.clone())
                }
            } else {
                Err(invalid_hash_error("preimage", &preimage.to_string()))
            }
        } else {
            Err(missing_parameter_error("preimage"))
        }
    } else {
        Err(invalid_input_error(&args.to_string()))
    }
}

pub async fn build_invoice_request(
    args: &serde_json::Value,
    plugin: &Plugin<PluginState>,
    rpc: &mut ClnRpc,
) -> Result<(String, Option<String>, Option<String>), anyhow::Error> {
    let cancel_hold_before_htlc_expiry_blocks = plugin
        .option(&OPT_CANCEL_HOLD_BEFORE_HTLC_EXPIRY_BLOCKS)
        .unwrap() as u32;

    let amount_msat = if let Some(amt) = args.get("amount_msat") {
        if let Some(amt_u64) = amt.as_u64() {
            if amt_u64 == 0 {
                return Err(invalid_amount_error());
            }
            amt_u64
        } else {
            return Err(invalid_integer_error(
                "amount_msat|msatoshi",
                &amt.to_string(),
            ));
        }
    } else {
        return Err(missing_parameter_error("amount_msat|msatoshi"));
    };

    let description = if let Some(desc) = args.get("description") {
        match desc {
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => s.as_str().to_owned(),
            e => return Err(invalid_input_error(&e.to_string())),
        }
    } else {
        return Err(missing_parameter_error("description"));
    };

    let expiry = if let Some(exp) = args.get("expiry") {
        let exp_u64 = match exp {
            serde_json::Value::Number(n) => n
                .as_u64()
                .ok_or_else(|| invalid_integer_error("expiry", &n.to_string()))?,
            serde_json::Value::String(s) => match s.parse::<u64>() {
                Ok(v) => v,
                Err(_) => return Err(invalid_integer_error("expiry", s)),
            },
            serde_json::Value::Null => DEFAULT_EXPIRY,
            e => return Err(invalid_input_error(&e.to_string())),
        };

        exp_u64
    } else {
        DEFAULT_EXPIRY
    };

    let mut preimage_str = if let Some(preimg) = args.get("preimage") {
        match preimg {
            serde_json::Value::Null => None,
            serde_json::Value::String(preimg_str) => {
                if preimg_str.len() != 64 {
                    return Err(invalid_hash_error("preimage", &preimg.to_string()));
                } else {
                    Some(preimg_str.to_owned())
                }
            }
            _ => return Err(invalid_hash_error("preimage", &preimg.to_string())),
        }
    } else {
        None
    };

    let payment_hash_str = if let Some(ph) = args.get("payment_hash") {
        match ph {
            serde_json::Value::Null => None,
            serde_json::Value::String(ph_str) => {
                if ph_str.len() != 64 {
                    return Err(invalid_hash_error("payment_hash", &ph.to_string()));
                } else {
                    Some(ph_str.to_owned())
                }
            }
            _ => return Err(invalid_hash_error("payment_hash", &ph.to_string())),
        }
    } else {
        None
    };

    if let Some(ref phash) = payment_hash_str {
        if let Some(ref preimg) = preimage_str {
            let preimg_hashed: sha256::Hash = Hash::hash(&hex::decode(preimg)?);
            if !phash.eq_ignore_ascii_case(&preimg_hashed.to_string()) {
                return Err(anyhow!(
                    "payment_hash: `{}` does not match preimage: `{}`",
                    phash,
                    preimg
                ));
            }
        }
    }

    let cltv = if let Some(c) = args.get("cltv") {
        let c_u64 = match c {
            serde_json::Value::Null => DEFAULT_CLTV_DELTA,
            serde_json::Value::Number(number) => number
                .as_u64()
                .ok_or_else(|| invalid_integer_error("cltv", &number.to_string()))?,
            serde_json::Value::String(c_str) => match c_str.parse::<u64>() {
                Ok(v) => v,
                Err(_) => return Err(invalid_integer_error("cltv", c_str)),
            },
            _ => return Err(invalid_integer_error("cltv", &c.to_string())),
        };

        if c_u64 as u32 <= cancel_hold_before_htlc_expiry_blocks {
            return Err(anyhow!(
                "cltv: needs to be greater than `{}` requested: `{}`",
                cancel_hold_before_htlc_expiry_blocks,
                c_u64
            ));
        } else {
            c_u64
        }
    } else {
        DEFAULT_CLTV_DELTA
    };

    let deschashonly = if let Some(dhash) = args.get("deschashonly") {
        match dhash {
            serde_json::Value::Null => false,
            serde_json::Value::Bool(b) => *b,
            serde_json::Value::String(b_str) => match b_str.parse::<bool>() {
                Ok(o) => o,
                Err(_) => {
                    return Err(anyhow!(
                        "deschashonly: should be `true` or `false`: \
                    invalid token `{}`",
                        b_str
                    ))
                }
            },
            _ => {
                return Err(anyhow!(
                    "deschashonly: should be `true` or `false`: \
                invalid token `{}`",
                    dhash.to_string()
                ))
            }
        }
    } else {
        false
    };

    let mut exposeprivatechannels = if let Some(expose) = args.get("exposeprivatechannels") {
        match expose {
            serde_json::Value::Null => None,
            serde_json::Value::Array(expose_arr) => {
                let mut scids = Vec::new();
                for scid_val in expose_arr {
                    if let Some(scid_str) = scid_val.as_str() {
                        if let Ok(scid) = ShortChannelId::from_str(scid_str) {
                            scids.push(scid)
                        } else {
                            return Err(invalid_scid_error(scid_str));
                        }
                    } else {
                        return Err(invalid_input_error(&scid_val.to_string()));
                    }
                }
                Some(scids)
            }
            _ => {
                return Err(anyhow!(
                    "exposeprivatechannels: should be an array: \
                invalid token `{}`",
                    expose.to_string()
                ))
            }
        }
    } else {
        None
    };
    if exposeprivatechannels.is_none() {
        let peer_channels = rpc
            .call_typed(&ListpeerchannelsRequest { id: None })
            .await?
            .channels;
        let mut online_public_channel_found = false;
        let mut private_channels = Vec::new();
        for channel in peer_channels.into_iter() {
            if let Some(private) = channel.private {
                if !private
                    && (channel.state == ChannelState::CHANNELD_NORMAL
                        || channel.state == ChannelState::CHANNELD_AWAITING_SPLICE)
                    && channel.peer_connected
                {
                    online_public_channel_found = true;
                    break;
                } else if private {
                    if let Some(scid) = channel.short_channel_id {
                        private_channels.push(scid);
                    }
                }
            }
        }
        if !online_public_channel_found {
            exposeprivatechannels = Some(private_channels)
        }
    }

    let payment_hash: sha256::Hash = if let Some(phash) = payment_hash_str {
        Hash::from_slice(&hex::decode(phash)?)?
    } else if let Some(ref preimg) = preimage_str {
        Hash::hash(&hex::decode(preimg)?)
    } else {
        let mut preimage = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut preimage[..]);
        preimage_str = Some(hex::encode(preimage));
        Hash::hash(&preimage)
    };

    let mut payment_secret_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut payment_secret_bytes[..]);
    let payment_secret = PaymentSecret(payment_secret_bytes);

    log::trace!(
        "NEW INVOICE: payment_hash: {}, preimage: {:?}",
        hex::encode(payment_hash),
        preimage_str
    );

    let invoice_builder = InvoiceBuilder::new(plugin.state().currency.clone())
        .amount_milli_satoshis(amount_msat)
        .payment_hash(payment_hash)
        .min_final_cltv_expiry_delta(cltv)
        .expiry_time(Duration::from_secs(expiry))
        .payment_secret(payment_secret)
        .basic_mpp()
        .current_timestamp();

    let mut invoice_builder = if deschashonly {
        invoice_builder.description_hash(Hash::hash(description.as_bytes()))
    } else {
        invoice_builder.description(description.clone())
    };

    if let Some(priv_chans) = exposeprivatechannels {
        let fake_label = "fake_invoice_for_route_gen".to_owned();
        let fake_invoice = rpc
            .call_typed(&InvoiceRequest {
                cltv: Some(cltv as u32),
                deschashonly: Some(deschashonly),
                expiry: Some(expiry),
                preimage: preimage_str.clone(),
                exposeprivatechannels: Some(priv_chans),
                fallbacks: None,
                amount_msat: cln_rpc::primitives::AmountOrAny::Amount(
                    cln_rpc::primitives::Amount::from_msat(amount_msat),
                ),
                description: description.clone(),
                label: fake_label.clone(),
            })
            .await?;
        rpc.call_typed(&DelinvoiceRequest {
            desconly: None,
            status: DelinvoiceStatus::UNPAID,
            label: fake_label,
        })
        .await?;

        let decoded_fake_invoice = rpc
            .call_typed(&DecodeRequest {
                string: fake_invoice.bolt11,
            })
            .await?;

        if let Some(routes) = decoded_fake_invoice.routes {
            for hint in routes.hints.into_iter() {
                let r_h: Vec<RouteHintHop> = hint
                    .hops
                    .into_iter()
                    .map(|h| RouteHintHop {
                        src_node_id: PublicKey::from_str(&h.pubkey.to_string()).unwrap(),
                        short_channel_id: h.short_channel_id.to_u64(),
                        fees: RoutingFees {
                            base_msat: h.fee_base_msat.msat() as u32,
                            proportional_millionths: h.fee_proportional_millionths,
                        },
                        cltv_expiry_delta: h.cltv_expiry_delta,
                        htlc_minimum_msat: None,
                        htlc_maximum_msat: None,
                    })
                    .collect();
                invoice_builder = invoice_builder.private_route(RouteHint(r_h));
            }
        }
    }

    let invoice = invoice_builder.build_raw()?.sign(|hash| {
        Ok::<bitcoin::secp256k1::ecdsa::RecoverableSignature, anyhow::Error>(
            Secp256k1::new().sign_ecdsa_recoverable(hash, &SecretKey::new(&mut rand::thread_rng())),
        )
    })?;

    let signed_invoice = rpc
        .call_typed(&SigninvoiceRequest {
            invstring: invoice.to_string(),
        })
        .await?;

    if deschashonly {
        Ok((signed_invoice.bolt11, preimage_str, Some(description)))
    } else {
        Ok((signed_invoice.bolt11, preimage_str, None))
    }
}
