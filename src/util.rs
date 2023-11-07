use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::model::PluginState;
use anyhow::anyhow;
use cln_plugin::{Error, Plugin};
use cln_rpc::model::requests::{
    DatastoreMode, DatastoreRequest, DeldatastoreRequest, ListdatastoreRequest,
    ListinvoicesRequest, ListpeerchannelsRequest,
};
use cln_rpc::{
    model::responses::{
        DatastoreResponse, DeldatastoreResponse, ListdatastoreDatastore, ListdatastoreResponse,
        ListinvoicesResponse, ListpeerchannelsResponse,
    },
    ClnRpc, Request, Response,
};

const HOLD_INVOICE_PLUGIN_NAME: &str = "holdinvoice";
const HOLD_INVOICE_DATASTORE_STATE: &str = "state";
const HOLD_INVOICE_DATASTORE_HTLC_EXPIRY: &str = "expiry";
pub const CANCEL_HOLD_BEFORE_INVOICE_EXPIRY_SECONDS: u64 = 1_800;
pub const CANCEL_HOLD_BEFORE_HTLC_EXPIRY_BLOCKS: u32 = 6;

use log::debug;

use crate::model::{HoldInvoice, HtlcIdentifier};

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

pub async fn listinvoices(
    rpc_path: &PathBuf,
    label: Option<String>,
    payment_hash: Option<String>,
) -> Result<ListinvoicesResponse, Error> {
    let mut rpc = ClnRpc::new(&rpc_path).await?;
    let invoice_request = rpc
        .call(Request::ListInvoices(ListinvoicesRequest {
            label,
            invstring: None,
            payment_hash,
            offer_id: None,
            index: None,
            start: None,
            limit: None,
        }))
        .await
        .map_err(|e| anyhow!("Error calling listinvoices: {:?}", e))?;
    match invoice_request {
        Response::ListInvoices(info) => Ok(info),
        e => Err(anyhow!("Unexpected result in listinvoices: {:?}", e)),
    }
}

pub async fn listpeerchannels(rpc_path: &PathBuf) -> Result<ListpeerchannelsResponse, Error> {
    let mut rpc = ClnRpc::new(&rpc_path).await?;
    let list_peer_channels = rpc
        .call(Request::ListPeerChannels(ListpeerchannelsRequest {
            id: None,
        }))
        .await
        .map_err(|e| anyhow!("Error calling listpeerchannels: {}", e.to_string()))?;
    match list_peer_channels {
        Response::ListPeerChannels(info) => Ok(info),
        e => Err(anyhow!("Unexpected result in listpeerchannels: {:?}", e)),
    }
}

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

async fn datastore_raw(
    rpc_path: &PathBuf,
    key: Vec<String>,
    string: Option<String>,
    hex: Option<String>,
    mode: Option<DatastoreMode>,
    generation: Option<u64>,
) -> Result<DatastoreResponse, Error> {
    let mut rpc = ClnRpc::new(&rpc_path).await?;
    let datastore_request = rpc
        .call(Request::Datastore(DatastoreRequest {
            key: key.clone(),
            string: string.clone(),
            hex,
            mode,
            generation,
        }))
        .await
        .map_err(|e| anyhow!("Error calling datastore: {:?}", e))?;
    debug!("datastore_raw: set {:?} to {}", key, string.unwrap());
    match datastore_request {
        Response::Datastore(info) => Ok(info),
        e => Err(anyhow!("Unexpected result in datastore: {:?}", e)),
    }
}

pub async fn datastore_new_state(
    rpc_path: &PathBuf,
    pay_hash: String,
    string: String,
) -> Result<DatastoreResponse, Error> {
    datastore_raw(
        rpc_path,
        vec![
            HOLD_INVOICE_PLUGIN_NAME.to_string(),
            pay_hash,
            HOLD_INVOICE_DATASTORE_STATE.to_string(),
        ],
        Some(string),
        None,
        Some(DatastoreMode::MUST_CREATE),
        None,
    )
    .await
}

pub async fn datastore_update_state(
    rpc_path: &PathBuf,
    pay_hash: String,
    string: String,
    generation: u64,
) -> Result<DatastoreResponse, Error> {
    datastore_raw(
        rpc_path,
        vec![
            HOLD_INVOICE_PLUGIN_NAME.to_string(),
            pay_hash,
            HOLD_INVOICE_DATASTORE_STATE.to_string(),
        ],
        Some(string),
        None,
        Some(DatastoreMode::MUST_REPLACE),
        Some(generation),
    )
    .await
}

pub async fn datastore_update_state_forced(
    rpc_path: &PathBuf,
    pay_hash: String,
    string: String,
) -> Result<DatastoreResponse, Error> {
    datastore_raw(
        rpc_path,
        vec![
            HOLD_INVOICE_PLUGIN_NAME.to_string(),
            pay_hash,
            HOLD_INVOICE_DATASTORE_STATE.to_string(),
        ],
        Some(string),
        None,
        Some(DatastoreMode::MUST_REPLACE),
        None,
    )
    .await
}

pub async fn datastore_htlc_expiry(
    rpc_path: &PathBuf,
    pay_hash: String,
    string: String,
) -> Result<DatastoreResponse, Error> {
    datastore_raw(
        rpc_path,
        vec![
            HOLD_INVOICE_PLUGIN_NAME.to_string(),
            pay_hash,
            HOLD_INVOICE_DATASTORE_HTLC_EXPIRY.to_string(),
        ],
        Some(string),
        None,
        Some(DatastoreMode::CREATE_OR_REPLACE),
        None,
    )
    .await
}

async fn listdatastore_raw(
    rpc_path: &PathBuf,
    key: Option<Vec<String>>,
) -> Result<ListdatastoreResponse, Error> {
    let mut rpc = ClnRpc::new(&rpc_path).await?;
    let datastore_request = rpc
        .call(Request::ListDatastore(ListdatastoreRequest { key }))
        .await
        .map_err(|e| anyhow!("Error calling listdatastore: {:?}", e))?;
    match datastore_request {
        Response::ListDatastore(info) => Ok(info),
        e => Err(anyhow!("Unexpected result in listdatastore: {:?}", e)),
    }
}

pub async fn listdatastore_all(rpc_path: &PathBuf) -> Result<ListdatastoreResponse, Error> {
    listdatastore_raw(rpc_path, Some(vec![HOLD_INVOICE_PLUGIN_NAME.to_string()])).await
}

pub async fn listdatastore_state(
    rpc_path: &PathBuf,
    pay_hash: String,
) -> Result<ListdatastoreDatastore, Error> {
    let response = listdatastore_raw(
        rpc_path,
        Some(vec![
            HOLD_INVOICE_PLUGIN_NAME.to_string(),
            pay_hash.clone(),
            HOLD_INVOICE_DATASTORE_STATE.to_string(),
        ]),
    )
    .await?;
    let data = response.datastore.first().ok_or_else(|| {
        anyhow!(
            "empty result for listdatastore_state with pay_hash: {}",
            pay_hash
        )
    })?;
    Ok(data.clone())
}

pub async fn listdatastore_htlc_expiry(rpc_path: &PathBuf, pay_hash: String) -> Result<u32, Error> {
    let response = listdatastore_raw(
        rpc_path,
        Some(vec![
            HOLD_INVOICE_PLUGIN_NAME.to_string(),
            pay_hash.clone(),
            HOLD_INVOICE_DATASTORE_HTLC_EXPIRY.to_string(),
        ]),
    )
    .await?;
    let data = response
        .datastore
        .first()
        .ok_or_else(|| {
            anyhow!(
                "empty result for listdatastore_htlc_expiry with pay_hash: {}",
                pay_hash
            )
        })?
        .string
        .as_ref()
        .ok_or_else(|| {
            anyhow!(
                "None string for listdatastore_htlc_expiry with pay_hash: {}",
                pay_hash
            )
        })?;
    let cltv = data.parse::<u32>()?;
    Ok(cltv)
}

async fn del_datastore_raw(
    rpc_path: &PathBuf,
    key: Vec<String>,
) -> Result<DeldatastoreResponse, Error> {
    let mut rpc = ClnRpc::new(&rpc_path).await?;
    let del_datastore_request = rpc
        .call(Request::DelDatastore(DeldatastoreRequest {
            key,
            generation: None,
        }))
        .await
        .map_err(|e| anyhow!("Error calling DelDatastore: {:?}", e))?;
    match del_datastore_request {
        Response::DelDatastore(info) => Ok(info),
        e => Err(anyhow!("Unexpected result in DelDatastore: {:?}", e)),
    }
}

pub async fn del_datastore_state(
    rpc_path: &PathBuf,
    pay_hash: String,
) -> Result<DeldatastoreResponse, Error> {
    del_datastore_raw(
        rpc_path,
        vec![
            HOLD_INVOICE_PLUGIN_NAME.to_string(),
            pay_hash,
            HOLD_INVOICE_DATASTORE_STATE.to_string(),
        ],
    )
    .await
}

pub async fn del_datastore_htlc_expiry(
    rpc_path: &PathBuf,
    pay_hash: String,
) -> Result<DeldatastoreResponse, Error> {
    del_datastore_raw(
        rpc_path,
        vec![
            HOLD_INVOICE_PLUGIN_NAME.to_string(),
            pay_hash.clone(),
            HOLD_INVOICE_DATASTORE_HTLC_EXPIRY.to_string(),
        ],
    )
    .await
}

pub fn short_channel_id_to_string(scid: u64) -> String {
    let block_height = scid >> 40;
    let tx_index = (scid >> 16) & 0xFFFFFF;
    let output_index = scid & 0xFFFF;
    format!("{}x{}x{}", block_height, tx_index, output_index)
}
