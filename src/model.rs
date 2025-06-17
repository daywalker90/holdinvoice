use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use crate::{pb, tls::Identity};
use anyhow::anyhow;
use cln_plugin::Error;
use cln_rpc::{primitives::ShortChannelId, ClnRpc};
use lightning_invoice::{Bolt11Invoice, Currency};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

pub const HOLD_INVOICE_PLUGIN_NAME: &str = "holdinvoice_v2";
pub const HOLD_STARTUP_LOCK: u64 = 20;
pub const DEFAULT_CLTV_DELTA: u64 = 144;
pub const DEFAULT_EXPIRY: u64 = 604_800;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Holdstate {
    Open,
    Settled,
    Canceled,
    Accepted,
}
impl Holdstate {
    pub fn as_i32(&self) -> i32 {
        match self {
            Holdstate::Open => 0,
            Holdstate::Settled => 1,
            Holdstate::Canceled => 2,
            Holdstate::Accepted => 3,
        }
    }
    pub fn is_valid_new_state(&self, newstate: &Holdstate) -> bool {
        match self {
            Holdstate::Open => !matches!(newstate, Holdstate::Open | Holdstate::Settled),
            Holdstate::Settled => false,
            Holdstate::Canceled => false,
            Holdstate::Accepted => true,
        }
    }
}
impl fmt::Display for Holdstate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Holdstate::Open => write!(f, "OPEN"),
            Holdstate::Settled => write!(f, "SETTLED"),
            Holdstate::Canceled => write!(f, "CANCELED"),
            Holdstate::Accepted => write!(f, "ACCEPTED"),
        }
    }
}
impl FromStr for Holdstate {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "open" => Ok(Holdstate::Open),
            "settled" => Ok(Holdstate::Settled),
            "canceled" => Ok(Holdstate::Canceled),
            "accepted" => Ok(Holdstate::Accepted),
            _ => Err(anyhow!("could not parse Holdstate from {}", s)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct HoldHtlc {
    pub amount_msat: u64,
    pub cltv_expiry: u32,
    pub loop_mutex: Arc<tokio::sync::Mutex<bool>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HtlcIdentifier {
    pub scid: ShortChannelId,
    pub htlc_id: u64,
}

#[derive(Clone, Debug)]
pub struct HoldInvoice {
    pub hold_state: Holdstate,
    pub generation: u64,
    pub htlc_data: HashMap<HtlcIdentifier, HoldHtlc>,
    pub invoice: Bolt11Invoice,
    pub preimage: Option<String>,
}

#[derive(Clone)]
pub struct PluginState {
    pub blockheight: Arc<Mutex<u32>>,
    pub holdinvoices: Arc<tokio::sync::Mutex<BTreeMap<String, HoldInvoice>>>,
    pub identity: Identity,
    pub ca_cert: Vec<u8>,
    pub method_rpc: Arc<tokio::sync::Mutex<ClnRpc>>,
    pub loop_rpc: Arc<tokio::sync::Mutex<ClnRpc>>,
    pub currency: Currency,
    pub startup_time: u64,
}

fn is_none_or_empty<T>(f: &Option<Vec<T>>) -> bool
where
    T: Clone,
{
    f.as_ref().map_or(true, |value| value.is_empty())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HoldInvoiceRequest {
    pub amount_msat: u64,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiry: Option<u64>,
    #[serde(skip_serializing_if = "is_none_or_empty")]
    pub exposeprivatechannels: Option<Vec<ShortChannelId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preimage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cltv: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deschashonly: Option<bool>,
}

impl From<HoldInvoiceRequest> for pb::HoldInvoiceRequest {
    fn from(c: HoldInvoiceRequest) -> Self {
        Self {
            amount_msat: Some(pb::Amount {
                msat: c.amount_msat,
            }), // Rule #2 for type msat_or_any
            description: c.description, // Rule #2 for type string
            expiry: c.expiry,           // Rule #2 for type u64?
            // Field: Invoice.fallbacks[]
            exposeprivatechannels: c
                .exposeprivatechannels
                .map(|arr| arr.into_iter().map(|i| i.to_string()).collect())
                .unwrap_or_default(), // Rule #3
            preimage: c.preimage.map(|v| hex::decode(v).unwrap()), // Rule #2 for type hex?
            payment_hash: c.payment_hash.map(|v| hex::decode(v).unwrap()), // Rule #2 for type hex?
            cltv: c.cltv,                                          // Rule #2 for type u32?
            deschashonly: c.deschashonly,                          // Rule #2 for type boolean?
        }
    }
}
impl From<pb::HoldInvoiceRequest> for HoldInvoiceRequest {
    fn from(c: pb::HoldInvoiceRequest) -> Self {
        Self {
            amount_msat: if let Some(amount) = c.amount_msat {
                amount.msat
            } else {
                0
            },
            description: c.description, // Rule #1 for type string
            expiry: c.expiry,           // Rule #1 for type u64?
            exposeprivatechannels: Some(
                c.exposeprivatechannels
                    .into_iter()
                    .map(|s| cln_rpc::primitives::ShortChannelId::from_str(&s).unwrap())
                    .collect(),
            ), // Rule #4
            preimage: c.preimage.map(hex::encode), // Rule #1 for type hex?
            payment_hash: c.payment_hash.map(hex::encode), // Rule #1 for type hex?
            cltv: c.cltv,               // Rule #1 for type u32?
            deschashonly: c.deschashonly, // Rule #1 for type boolean?
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HoldInvoiceResponse {
    pub bolt11: String,
    pub payment_hash: String,
    pub payment_secret: String,
    pub expires_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preimage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description_hash: Option<String>,
    pub state: Holdstate,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub htlc_expiry: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paid_at: Option<u64>,
}

impl From<HoldInvoiceResponse> for pb::HoldInvoiceResponse {
    fn from(c: HoldInvoiceResponse) -> Self {
        Self {
            bolt11: c.bolt11,
            payment_hash: hex::decode(c.payment_hash).unwrap(),
            payment_secret: hex::decode(c.payment_secret).unwrap(),
            expires_at: c.expires_at,
            preimage: c.preimage.map(|v| hex::decode(v).unwrap()),
            description: c.description,
            description_hash: c.description_hash.map(|v| hex::decode(v).unwrap()),
            state: c.state.as_i32(),
            htlc_expiry: c.htlc_expiry,
            paid_at: c.paid_at,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HoldLookupResponse {
    pub holdinvoices: Vec<HoldInvoiceResponse>,
}

impl From<HoldLookupResponse> for pb::HoldInvoiceLookupResponse {
    fn from(c: HoldLookupResponse) -> Self {
        Self {
            holdinvoices: c.holdinvoices.into_iter().map(|hi| hi.into()).collect(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HoldStateResponse {
    pub state: Holdstate,
}
