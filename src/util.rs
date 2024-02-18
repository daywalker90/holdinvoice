use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::model::PluginState;
use crate::{
    errors::*, OPT_CANCEL_HOLD_BEFORE_HTLC_EXPIRY_BLOCKS,
    OPT_CANCEL_HOLD_BEFORE_INVOICE_EXPIRY_SECONDS,
};

use cln_plugin::{Error, Plugin};
use cln_rpc::model::requests::InvoiceRequest;
use cln_rpc::primitives::{Amount, AmountOrAny, ShortChannelId};
use serde_json::json;

use crate::model::{HoldInvoice, HtlcIdentifier};

pub fn make_rpc_path(plugin: Plugin<PluginState>) -> PathBuf {
    Path::new(&plugin.configuration().lightning_dir).join(plugin.configuration().rpc_file)
}

pub fn u64_to_scid(scid: u64) -> Result<ShortChannelId, Error> {
    let block_height = scid >> 40;
    let tx_index = (scid >> 16) & 0xFFFFFF;
    let output_index = scid & 0xFFFF;
    ShortChannelId::from_str(&format!("{}x{}x{}", block_height, tx_index, output_index))
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

pub fn parse_payment_hash(args: serde_json::Value) -> Result<String, serde_json::Value> {
    if let serde_json::Value::Array(i) = args {
        if i.is_empty() {
            Err(missing_parameter_error("payment_hash"))
        } else if i.len() != 1 {
            Err(too_many_params_error(i.len(), 1))
        } else if let serde_json::Value::String(s) = i.first().unwrap() {
            if s.len() != 64 {
                Err(invalid_hash_error("payment_hash", s))
            } else {
                Ok(s.clone())
            }
        } else {
            Err(invalid_hash_error(
                "payment_hash",
                &i.first().unwrap().to_string(),
            ))
        }
    } else if let serde_json::Value::Object(o) = args {
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

pub fn build_invoice_request(
    args: &serde_json::Value,
    plugin: &Plugin<PluginState>,
) -> Result<InvoiceRequest, serde_json::Value> {
    let cancel_hold_before_invoice_expiry_seconds = plugin
        .option(&OPT_CANCEL_HOLD_BEFORE_INVOICE_EXPIRY_SECONDS)
        .unwrap() as u64;
    let cancel_hold_before_htlc_expiry_blocks = plugin
        .option(&OPT_CANCEL_HOLD_BEFORE_HTLC_EXPIRY_BLOCKS)
        .unwrap() as u32;
    let amount_msat = if let Some(amt) = args.get("amount_msat") {
        AmountOrAny::Amount(Amount::from_msat(if let Some(amt_u64) = amt.as_u64() {
            amt_u64
        } else {
            return Err(invalid_integer_error(
                "amount_msat|msatoshi",
                &amt.to_string(),
            ));
        }))
    } else {
        return Err(missing_parameter_error("amount_msat|msatoshi"));
    };

    let label = if let Some(lbl) = args.get("label") {
        match lbl {
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => s.as_str().to_string(),
            e => return Err(invalid_input_error(&e.to_string())),
        }
    } else {
        return Err(missing_parameter_error("label"));
    };

    let description = if let Some(desc) = args.get("description") {
        match desc {
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => s.as_str().to_string(),
            e => return Err(invalid_input_error(&e.to_string())),
        }
    } else {
        return Err(missing_parameter_error("description"));
    };

    let expiry = if let Some(exp) = args.get("expiry") {
        Some(if let Some(exp_u64) = exp.as_u64() {
            if exp_u64 <= cancel_hold_before_invoice_expiry_seconds {
                return Err(json!({
                    "code": -32602,
                    "message": format!("expiry: needs to be greater than '{}' requested: '{}'",
                    cancel_hold_before_invoice_expiry_seconds, exp_u64)
                }));
            } else {
                exp_u64
            }
        } else {
            return Err(invalid_integer_error("expiry", &exp.to_string()));
        })
    } else {
        None
    };

    let fallbacks = if let Some(fbcks) = args.get("fallbacks") {
        Some(if let Some(fbcks_arr) = fbcks.as_array() {
            fbcks_arr
                .iter()
                .filter_map(|value| value.as_str().map(|s| s.to_string()))
                .collect()
        } else {
            return Err(json!({
                "code": -32602,
                "message": format!("fallbacks: should be an array: \
                invalid token '{}'", fbcks.to_string())
            }));
        })
    } else {
        None
    };

    let preimage = if let Some(preimg) = args.get("preimage") {
        Some(if let Some(preimg_str) = preimg.as_str() {
            if preimg_str.len() != 64 {
                return Err(invalid_hash_error("preimage", &preimg.to_string()));
            } else {
                preimg_str.to_string()
            }
        } else {
            return Err(invalid_hash_error("preimage", &preimg.to_string()));
        })
    } else {
        None
    };

    let cltv = if let Some(c) = args.get("cltv") {
        Some(if let Some(c_u64) = c.as_u64() {
            if c_u64 as u32 <= cancel_hold_before_htlc_expiry_blocks {
                return Err(json!({
                    "code": -32602,
                    "message": format!("cltv: needs to be greater than '{}' requested: '{}'",
                    cancel_hold_before_htlc_expiry_blocks, c_u64)
                }));
            } else {
                c_u64 as u32
            }
        } else {
            return Err(json!({
                "code": -32602,
                "message": format!("cltv: should be an integer: \
                invalid token '{}'", c.to_string())
            }));
        })
    } else {
        return Err(missing_parameter_error("cltv"));
    };

    let deschashonly = if let Some(dhash) = args.get("deschashonly") {
        Some(if let Some(dhash_bool) = dhash.as_bool() {
            dhash_bool
        } else {
            return Err(json!({
                "code": -32602,
                "message": format!("deschashonly: should be 'true' or 'false': \
                invalid token '{}'", dhash.to_string())
            }));
        })
    } else {
        None
    };

    Ok(InvoiceRequest {
        amount_msat,
        label,
        description,
        expiry,
        fallbacks,
        preimage,
        cltv,
        deschashonly,
    })
}
