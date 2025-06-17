use cln_rpc::model::requests::{
    DatastoreMode, DatastoreRequest, DeldatastoreRequest, ListdatastoreRequest,
};
use cln_rpc::RpcError;
use cln_rpc::{model::responses::DeldatastoreResponse, ClnRpc};

use crate::model::{HoldInvoiceResponse, Holdstate, HOLD_INVOICE_PLUGIN_NAME};

pub async fn datastore_new_hold_invoice(
    rpc: &mut ClnRpc,
    payment_hash: String,
    invoice: HoldInvoiceResponse,
) -> Result<(), RpcError> {
    rpc.call_typed(&DatastoreRequest {
        generation: None,
        hex: Some(hex::encode(serde_json::to_vec(&invoice).unwrap())),
        mode: Some(DatastoreMode::MUST_CREATE),
        string: None,
        key: vec![HOLD_INVOICE_PLUGIN_NAME.to_owned(), payment_hash.clone()],
    })
    .await?;

    Ok(())
}

pub async fn datastore_update_hold_invoice(
    rpc: &mut ClnRpc,
    payment_hash: String,
    invoice: HoldInvoiceResponse,
    generation: u64,
) -> Result<(), RpcError> {
    rpc.call_typed(&DatastoreRequest {
        generation: Some(generation),
        hex: Some(hex::encode(serde_json::to_vec(&invoice).unwrap())),
        mode: Some(DatastoreMode::MUST_REPLACE),
        string: None,
        key: vec![HOLD_INVOICE_PLUGIN_NAME.to_owned(), payment_hash.clone()],
    })
    .await?;

    Ok(())
}

pub async fn datastore_update_hold_invoice_forced(
    rpc: &mut ClnRpc,
    payment_hash: String,
    invoice: HoldInvoiceResponse,
) -> Result<(), RpcError> {
    rpc.call_typed(&DatastoreRequest {
        generation: None,
        hex: Some(hex::encode(serde_json::to_vec(&invoice).unwrap())),
        mode: Some(DatastoreMode::MUST_REPLACE),
        string: None,
        key: vec![HOLD_INVOICE_PLUGIN_NAME.to_owned(), payment_hash.clone()],
    })
    .await?;

    Ok(())
}

pub async fn datastore_update_state(
    rpc: &mut ClnRpc,
    pay_hash: String,
    state: Holdstate,
    generation_old: u64,
) -> Result<(), RpcError> {
    let (mut old_invoice, generation_now) = listdatastore_payment_hash(rpc, &pay_hash).await?;
    if generation_now != generation_old {
        return Err(RpcError {
            code: Some(-32700),
            message: "The generation was wrong".to_string(),
            data: None,
        });
    }
    old_invoice.state = state;
    datastore_update_hold_invoice(rpc, pay_hash, old_invoice, generation_now).await
}

pub async fn datastore_update_state_forced(
    rpc: &mut ClnRpc,
    pay_hash: String,
    state: Holdstate,
) -> Result<(), RpcError> {
    let (mut old_invoice, _generation) = listdatastore_payment_hash(rpc, &pay_hash).await?;
    old_invoice.state = state;
    datastore_update_hold_invoice_forced(rpc, pay_hash, old_invoice).await
}

pub async fn listdatastore_all(rpc: &mut ClnRpc) -> Result<Vec<HoldInvoiceResponse>, RpcError> {
    let datastore = rpc
        .call_typed(&ListdatastoreRequest {
            key: Some(vec![HOLD_INVOICE_PLUGIN_NAME.to_owned()]),
        })
        .await?;

    let mut holdinvoices = Vec::with_capacity(datastore.datastore.len());

    for data in datastore.datastore.iter() {
        let (holdinvoice, _generation) = listdatastore_payment_hash(rpc, &data.key[1]).await?;
        holdinvoices.push(holdinvoice);
    }
    Ok(holdinvoices)
}

pub async fn listdatastore_payment_hash(
    rpc: &mut ClnRpc,
    payment_hash: &str,
) -> Result<(HoldInvoiceResponse, u64), RpcError> {
    let key = vec![HOLD_INVOICE_PLUGIN_NAME.to_owned(), payment_hash.to_owned()];
    let lookup = rpc
        .call_typed(&ListdatastoreRequest { key: Some(key) })
        .await?;

    if lookup.datastore.is_empty() {
        return Err(RpcError {
            message: "payment_hash not found in database".to_owned(),
            code: Some(-32700),
            data: None,
        });
    }

    let data = lookup.datastore.first().ok_or_else(|| RpcError {
        message: format!(
            "empty result for listdatastore_payment_hash with pay_hash: {}",
            payment_hash
        ),
        code: Some(-32700),
        data: None,
    })?;

    let response = if let Some(hx) = &data.hex {
        serde_json::from_slice(&hex::decode(hx).map_err(|e| RpcError {
            message: e.to_string(),
            code: Some(-32700),
            data: None,
        })?)
        .map_err(|e| RpcError {
            message: e.to_string(),
            code: Some(-32700),
            data: None,
        })?
    } else {
        return Err(RpcError {
            message: format!("hex data mssing for payment_hash: {}", payment_hash),
            code: Some(-32700),
            data: None,
        });
    };

    Ok((response, data.generation.unwrap_or(0)))
}

pub async fn del_datastore_hold_invoice(
    rpc: &mut ClnRpc,
    pay_hash: String,
) -> Result<DeldatastoreResponse, RpcError> {
    rpc.call_typed(&DeldatastoreRequest {
        generation: None,
        key: vec![HOLD_INVOICE_PLUGIN_NAME.to_owned(), pay_hash],
    })
    .await
}
