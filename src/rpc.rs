use anyhow::anyhow;
use cln_plugin::Error;
use cln_rpc::{
    model::{
        requests::{DatastoreMode, DatastoreRequest, DeldatastoreRequest, ListdatastoreRequest},
        responses::{
            DatastoreResponse,
            DeldatastoreResponse,
            ListdatastoreDatastore,
            ListdatastoreResponse,
        },
    },
    ClnRpc,
    RpcError,
};

use crate::model::{HOLD_INVOICE_DATASTORE_STATE, HOLD_INVOICE_PLUGIN_NAME};

pub async fn datastore_new_state(
    rpc: &mut ClnRpc,
    pay_hash: String,
    string: String,
) -> Result<DatastoreResponse, RpcError> {
    rpc.call_typed(&DatastoreRequest {
        generation: None,
        hex: None,
        mode: Some(DatastoreMode::MUST_CREATE),
        string: Some(string),
        key: vec![
            HOLD_INVOICE_PLUGIN_NAME.to_owned(),
            pay_hash,
            HOLD_INVOICE_DATASTORE_STATE.to_owned(),
        ],
    })
    .await
}

pub async fn datastore_update_state(
    rpc: &mut ClnRpc,
    pay_hash: String,
    string: String,
    generation: u64,
) -> Result<DatastoreResponse, RpcError> {
    rpc.call_typed(&DatastoreRequest {
        generation: Some(generation),
        hex: None,
        mode: Some(DatastoreMode::MUST_REPLACE),
        string: Some(string),
        key: vec![
            HOLD_INVOICE_PLUGIN_NAME.to_owned(),
            pay_hash,
            HOLD_INVOICE_DATASTORE_STATE.to_owned(),
        ],
    })
    .await
}

pub async fn datastore_update_state_forced(
    rpc: &mut ClnRpc,
    pay_hash: String,
    string: String,
) -> Result<DatastoreResponse, RpcError> {
    rpc.call_typed(&DatastoreRequest {
        generation: None,
        hex: None,
        mode: Some(DatastoreMode::MUST_REPLACE),
        string: Some(string),
        key: vec![
            HOLD_INVOICE_PLUGIN_NAME.to_owned(),
            pay_hash,
            HOLD_INVOICE_DATASTORE_STATE.to_owned(),
        ],
    })
    .await
}

pub async fn listdatastore_all(rpc: &mut ClnRpc) -> Result<ListdatastoreResponse, RpcError> {
    rpc.call_typed(&ListdatastoreRequest {
        key: Some(vec![HOLD_INVOICE_PLUGIN_NAME.to_owned()]),
    })
    .await
}

pub async fn listdatastore_state(
    rpc: &mut ClnRpc,
    pay_hash: String,
) -> Result<ListdatastoreDatastore, Error> {
    let response = rpc
        .call_typed(&ListdatastoreRequest {
            key: Some(vec![
                HOLD_INVOICE_PLUGIN_NAME.to_owned(),
                pay_hash.clone(),
                HOLD_INVOICE_DATASTORE_STATE.to_owned(),
            ]),
        })
        .await?;
    let data = response.datastore.first().ok_or_else(|| {
        anyhow!(
            "empty result for listdatastore_state with pay_hash: {}",
            pay_hash
        )
    })?;
    Ok(data.clone())
}

pub async fn del_datastore_state(
    rpc: &mut ClnRpc,
    pay_hash: String,
) -> Result<DeldatastoreResponse, RpcError> {
    rpc.call_typed(&DeldatastoreRequest {
        generation: None,
        key: vec![
            HOLD_INVOICE_PLUGIN_NAME.to_owned(),
            pay_hash,
            HOLD_INVOICE_DATASTORE_STATE.to_owned(),
        ],
    })
    .await
}
