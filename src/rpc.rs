use std::path::PathBuf;

use anyhow::anyhow;
use cln_plugin::Error;
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
use log::debug;

use crate::model::{
    HOLD_INVOICE_DATASTORE_HTLC_EXPIRY, HOLD_INVOICE_DATASTORE_STATE, HOLD_INVOICE_PLUGIN_NAME,
};

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
