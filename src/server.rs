use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::Result;
use cln_plugin::Plugin;
use tonic::{Code, Status};

use crate::{
    hold::{hold_invoice, hold_invoice_cancel, hold_invoice_lookup, hold_invoice_settle},
    model::{self, Holdstate, PluginState},
    pb,
    pb::hold_server::Hold,
};

#[derive(Clone)]
#[allow(dead_code)]
pub struct Server {
    rpc_path: PathBuf,
    plugin: Plugin<PluginState>,
}

impl Server {
    pub fn new(path: &Path, plugin: Plugin<PluginState>) -> Self {
        Self {
            rpc_path: path.to_path_buf(),
            plugin,
        }
    }
}

#[tonic::async_trait]
impl Hold for Server {
    async fn hold_invoice(
        &self,
        request: tonic::Request<pb::HoldInvoiceRequest>,
    ) -> Result<tonic::Response<pb::HoldInvoiceResponse>, tonic::Status> {
        let req = request.into_inner();
        let req: model::HoldInvoiceRequest = req.into();
        log::debug!("Client asked for Holdinvoice");
        log::trace!("Holdinvoice request: {req:?}");
        let result =
            match hold_invoice(self.plugin.clone(), serde_json::to_value(req).unwrap()).await {
                Ok(res) => res,
                Err(e) => {
                    return Err(Status::new(
                        Code::Internal,
                        format!("Unexpected result {e} to method call hold_invoice"),
                    ));
                }
            };
        log::debug!("{result:?}");
        match serde_json::from_value::<model::HoldInvoiceResponse>(result.clone()) {
            Ok(r) => {
                log::trace!("Holdinvoice response: {r:?}");
                Ok(tonic::Response::new(r.into()))
            }
            Err(_r) => Err(Status::new(
                Code::Internal,
                format!("Unexpected result {result} to method call HoldInvoice"),
            )),
        }
    }

    async fn hold_invoice_settle(
        &self,
        request: tonic::Request<pb::HoldInvoiceSettleRequest>,
    ) -> Result<tonic::Response<pb::HoldInvoiceSettleResponse>, tonic::Status> {
        let req = request.into_inner();
        log::debug!("Client asked for Holdinvoicesettle");
        log::debug!("Holdinvoicesettle request: {req:?}");
        let pay_hash = hex::encode(req.payment_hash.clone());
        log::debug!("payment_hash: {pay_hash}");
        let result = match hold_invoice_settle(
            self.plugin.clone(),
            serde_json::Value::Array(vec![serde_json::Value::String(pay_hash)]),
        )
        .await
        {
            Ok(res) => res,
            Err(e) => {
                return Err(Status::new(
                    Code::Internal,
                    format!("Unexpected result {e} to method call hold_invoice_settle"),
                ));
            }
        };

        if let Some(_err) = result.get("code") {
            Err(Status::new(
                Code::Internal,
                format!("Unexpected result {result} to method call hold_invoice_settle"),
            ))
        } else {
            if let Some(state) = result.get("state") {
                if let Ok(hs) = Holdstate::from_str(state.as_str().unwrap()) {
                    let hisr = pb::HoldInvoiceSettleResponse { state: hs.as_i32() };
                    return Ok(tonic::Response::new(hisr));
                }
            }
            Err(Status::new(
                Code::Internal,
                format!("Unexpected result {result} to method call hold_invoice_settle"),
            ))
        }
    }

    async fn hold_invoice_cancel(
        &self,
        request: tonic::Request<pb::HoldInvoiceCancelRequest>,
    ) -> Result<tonic::Response<pb::HoldInvoiceCancelResponse>, tonic::Status> {
        let req = request.into_inner();
        log::debug!("Client asked for Holdinvoicecancel");
        log::debug!("Holdinvoicecancel request: {req:?}");
        let pay_hash = hex::encode(req.payment_hash.clone());
        log::debug!("payment_hash: {pay_hash}");
        let result = match hold_invoice_cancel(
            self.plugin.clone(),
            serde_json::Value::Array(vec![serde_json::Value::String(pay_hash)]),
        )
        .await
        {
            Ok(res) => res,
            Err(e) => {
                return Err(Status::new(
                    Code::Internal,
                    format!("Unexpected result {e} to method call hold_invoice_cancel"),
                ));
            }
        };

        if let Some(_err) = result.get("code") {
            Err(Status::new(
                Code::Internal,
                format!("Unexpected result {result} to method call hold_invoice_cancel"),
            ))
        } else {
            if let Some(state) = result.get("state") {
                if let Ok(hs) = Holdstate::from_str(state.as_str().unwrap()) {
                    let hisr = pb::HoldInvoiceCancelResponse { state: hs.as_i32() };
                    return Ok(tonic::Response::new(hisr));
                }
            }
            Err(Status::new(
                Code::Internal,
                format!("Unexpected result {result} to method call hold_invoice_cancel"),
            ))
        }
    }

    async fn hold_invoice_lookup(
        &self,
        request: tonic::Request<pb::HoldInvoiceLookupRequest>,
    ) -> Result<tonic::Response<pb::HoldInvoiceLookupResponse>, tonic::Status> {
        let req = request.into_inner();
        log::debug!("Client asked for Holdinvoicelookup");
        log::debug!("Holdinvoicelookup request: {req:?}");
        let pay_hash = hex::encode(req.payment_hash.clone());
        log::debug!("payment_hash: {pay_hash}");
        let result = match hold_invoice_lookup(
            self.plugin.clone(),
            serde_json::Value::Array(vec![serde_json::Value::String(pay_hash)]),
        )
        .await
        {
            Ok(res) => res,
            Err(e) => {
                return Err(Status::new(
                    Code::Internal,
                    format!("Unexpected result {e} to method call hold_invoice_cancel"),
                ));
            }
        };

        if let Some(_err) = result.get("code") {
            Err(Status::new(
                Code::Internal,
                format!("Unexpected result {result} to method call hold_invoice_cancel"),
            ))
        } else {
            if let Some(state) = result.get("state") {
                if let Ok(hs) = Holdstate::from_str(state.as_str().unwrap()) {
                    log::debug!("hs.as_i32:{} hs:{}", hs.as_i32(), hs);
                    let hisr = pb::HoldInvoiceLookupResponse {
                        state: hs.as_i32(),
                        htlc_expiry: if hs == Holdstate::Accepted {
                            Some(
                                u32::try_from(result.get("htlc_expiry").unwrap().as_u64().unwrap())
                                    .unwrap(),
                            )
                        } else {
                            None
                        },
                    };
                    return Ok(tonic::Response::new(hisr));
                }
            }
            Err(Status::new(
                Code::Internal,
                format!("Unexpected result {result} to method call hold_invoice_cancel"),
            ))
        }
    }
}
