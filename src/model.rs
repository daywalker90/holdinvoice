use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use crate::{pb, tls::Identity};
use anyhow::anyhow;
use bitcoin::hashes::sha256::Hash as Sha256;
use cln_plugin::Error;
use cln_rpc::{
    model::responses::ListinvoicesInvoices,
    primitives::{Secret, ShortChannelId},
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

pub const HOLD_INVOICE_PLUGIN_NAME: &str = "holdinvoice";
pub const HOLD_INVOICE_DATASTORE_STATE: &str = "state";
pub const HOLD_INVOICE_DATASTORE_HTLC_EXPIRY: &str = "expiry";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
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
    pub fn is_valid_transition(&self, newstate: &Holdstate) -> bool {
        match self {
            Holdstate::Open => !matches!(newstate, Holdstate::Settled),
            Holdstate::Settled => matches!(newstate, Holdstate::Settled),
            Holdstate::Canceled => matches!(newstate, Holdstate::Canceled),
            Holdstate::Accepted => !matches!(newstate, Holdstate::Open),
        }
    }
}
impl fmt::Display for Holdstate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Holdstate::Open => write!(f, "open"),
            Holdstate::Settled => write!(f, "settled"),
            Holdstate::Canceled => write!(f, "canceled"),
            Holdstate::Accepted => write!(f, "accepted"),
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HtlcIdentifier {
    pub scid: ShortChannelId,
    pub htlc_id: u64,
}

#[derive(Clone, Debug)]
pub struct HoldInvoice {
    pub hold_state: Holdstate,
    pub generation: u64,
    pub htlc_data: HashMap<HtlcIdentifier, HoldHtlc>,
    pub last_htlc_expiry: u32,
    pub invoice: ListinvoicesInvoices,
}

#[derive(Clone, Debug)]
pub struct PluginState {
    pub blockheight: Arc<Mutex<u32>>,
    pub holdinvoices: Arc<tokio::sync::Mutex<BTreeMap<String, HoldInvoice>>>,
    pub identity: Identity,
    pub ca_cert: Vec<u8>,
}

fn is_none_or_empty<T>(f: &Option<Vec<T>>) -> bool
where
    T: Clone,
{
    f.as_ref().map_or(true, |value| value.is_empty())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
#[serde(rename_all = "lowercase")]
pub enum Request {
    HoldInvoice(HoldInvoiceRequest),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "method", content = "result")]
#[serde(rename_all = "lowercase")]
pub enum Response {
    HoldInvoice(HoldInvoiceResponse),
}

pub trait IntoRequest: Into<Request> {
    type Response: TryFrom<Response, Error = TryFromResponseError>;
}

#[derive(Debug)]
pub struct TryFromResponseError;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HoldInvoiceRequest {
    pub amount_msat: u64,
    pub description: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiry: Option<u64>,
    #[serde(skip_serializing_if = "is_none_or_empty")]
    pub fallbacks: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preimage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cltv: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deschashonly: Option<bool>,
}

impl From<HoldInvoiceRequest> for Request {
    fn from(r: HoldInvoiceRequest) -> Self {
        Request::HoldInvoice(r)
    }
}

impl IntoRequest for HoldInvoiceRequest {
    type Response = HoldInvoiceResponse;
}
#[allow(unused_variables, deprecated)]
impl From<HoldInvoiceRequest> for pb::HoldInvoiceRequest {
    fn from(c: HoldInvoiceRequest) -> Self {
        Self {
            amount_msat: Some(pb::Amount {
                msat: c.amount_msat,
            }), // Rule #2 for type msat_or_any
            description: c.description, // Rule #2 for type string
            label: c.label,             // Rule #2 for type string
            expiry: c.expiry,           // Rule #2 for type u64?
            // Field: Invoice.fallbacks[]
            fallbacks: c
                .fallbacks
                .map(|arr| arr.into_iter().collect())
                .unwrap_or_default(), // Rule #3
            preimage: c.preimage.map(|v| hex::decode(v).unwrap()), // Rule #2 for type hex?
            cltv: c.cltv,                                          // Rule #2 for type u32?
            deschashonly: c.deschashonly,                          // Rule #2 for type boolean?
        }
    }
}
#[allow(unused_variables, deprecated)]
impl From<pb::HoldInvoiceRequest> for HoldInvoiceRequest {
    fn from(c: pb::HoldInvoiceRequest) -> Self {
        Self {
            amount_msat: if let Some(amount) = c.amount_msat {
                amount.msat
            } else {
                0
            },
            description: c.description, // Rule #1 for type string
            label: c.label,             // Rule #1 for type string
            expiry: c.expiry,           // Rule #1 for type u64?
            fallbacks: Some(c.fallbacks.into_iter().collect()), // Rule #4
            preimage: c.preimage.map(hex::encode), // Rule #1 for type hex?
            cltv: c.cltv,               // Rule #1 for type u32?
            deschashonly: c.deschashonly, // Rule #1 for type boolean?
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HoldInvoiceResponse {
    pub bolt11: String,
    pub payment_hash: Sha256,
    pub payment_secret: Secret,
    pub expires_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning_capacity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning_offline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning_deadends: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning_private_unused: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning_mpp: Option<String>,
}

#[allow(unreachable_patterns)]
impl TryFrom<Response> for HoldInvoiceResponse {
    type Error = TryFromResponseError;

    fn try_from(response: Response) -> Result<Self, Self::Error> {
        match response {
            Response::HoldInvoice(response) => Ok(response),
            _ => Err(TryFromResponseError),
        }
    }
}

#[allow(unused_variables, deprecated)]
impl From<HoldInvoiceResponse> for pb::HoldInvoiceResponse {
    fn from(c: HoldInvoiceResponse) -> Self {
        Self {
            bolt11: c.bolt11, // Rule #2 for type string
            payment_hash: <Sha256 as AsRef<[u8]>>::as_ref(&c.payment_hash).to_vec(), // Rule #2 for type hash
            payment_secret: c.payment_secret.to_vec(), // Rule #2 for type secret
            expires_at: c.expires_at,                  // Rule #2 for type u64
            warning_capacity: c.warning_capacity,      // Rule #2 for type string?
            warning_offline: c.warning_offline,        // Rule #2 for type string?
            warning_deadends: c.warning_deadends,      // Rule #2 for type string?
            warning_private_unused: c.warning_private_unused, // Rule #2 for type string?
            warning_mpp: c.warning_mpp,                // Rule #2 for type string?
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HoldLookupResponse {
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub htlc_expiry: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HoldStateResponse {
    pub state: String,
}
