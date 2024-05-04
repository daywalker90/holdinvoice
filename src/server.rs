use crate::hold::{hold_invoice, hold_invoice_cancel, hold_invoice_lookup, hold_invoice_settle};
use crate::model::{self, Holdstate, PluginState};
use crate::pb;
use crate::pb::hold_server::Hold;
use anyhow::Result;
use cln_plugin::Plugin;
use log::{debug, trace};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tonic::{Code, Status};

#[derive(Clone)]
#[allow(dead_code)]
pub struct Server {
    rpc_path: PathBuf,
    plugin: Plugin<PluginState>,
}

impl Server {
    pub async fn new(path: &Path, plugin: Plugin<PluginState>) -> Result<Self> {
        Ok(Self {
            rpc_path: path.to_path_buf(),
            plugin,
        })
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
        debug!("Client asked for Holdinvoice");
        trace!("Holdinvoice request: {:?}", req);
        let result =
            match hold_invoice(self.plugin.clone(), serde_json::to_value(req).unwrap()).await {
                Ok(res) => res,
                Err(e) => {
                    return Err(Status::new(
                        Code::Internal,
                        format!("Unexpected result {} to method call hold_invoice", e),
                    ));
                }
            };
        debug!("{:?}", result);
        match serde_json::from_value::<model::HoldInvoiceResponse>(result.clone()) {
            Ok(r) => {
                trace!("Holdinvoice response: {:?}", r);
                Ok(tonic::Response::new(r.into()))
            }
            Err(_r) => Err(Status::new(
                Code::Internal,
                format!("Unexpected result {} to method call HoldInvoice", result),
            )),
        }
    }

    async fn hold_invoice_settle(
        &self,
        request: tonic::Request<pb::HoldInvoiceSettleRequest>,
    ) -> Result<tonic::Response<pb::HoldInvoiceSettleResponse>, tonic::Status> {
        let req = request.into_inner();
        debug!("Client asked for Holdinvoicesettle");
        debug!("Holdinvoicesettle request: {:?}", req);
        let pay_hash = hex::encode(req.payment_hash.clone());
        debug!("payment_hash: {}", pay_hash);
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
                    format!("Unexpected result {} to method call hold_invoice_settle", e),
                ));
            }
        };

        match result.get("code") {
            Some(_err) => Err(Status::new(
                Code::Internal,
                format!(
                    "Unexpected result {} to method call hold_invoice_settle",
                    result
                ),
            )),
            None => {
                if let Some(state) = result.get("state") {
                    if let Ok(hs) = Holdstate::from_str(state.as_str().unwrap()) {
                        let hisr = pb::HoldInvoiceSettleResponse { state: hs.as_i32() };
                        return Ok(tonic::Response::new(hisr));
                    }
                }
                Err(Status::new(
                    Code::Internal,
                    format!(
                        "Unexpected result {} to method call hold_invoice_settle",
                        result
                    ),
                ))
            }
        }
    }

    async fn hold_invoice_cancel(
        &self,
        request: tonic::Request<pb::HoldInvoiceCancelRequest>,
    ) -> Result<tonic::Response<pb::HoldInvoiceCancelResponse>, tonic::Status> {
        let req = request.into_inner();
        debug!("Client asked for Holdinvoicecancel");
        debug!("Holdinvoicecancel request: {:?}", req);
        let pay_hash = hex::encode(req.payment_hash.clone());
        debug!("payment_hash: {}", pay_hash);
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
                    format!("Unexpected result {} to method call hold_invoice_cancel", e),
                ));
            }
        };

        match result.get("code") {
            Some(_err) => Err(Status::new(
                Code::Internal,
                format!(
                    "Unexpected result {} to method call hold_invoice_cancel",
                    result
                ),
            )),
            None => {
                if let Some(state) = result.get("state") {
                    if let Ok(hs) = Holdstate::from_str(state.as_str().unwrap()) {
                        let hisr = pb::HoldInvoiceCancelResponse { state: hs.as_i32() };
                        return Ok(tonic::Response::new(hisr));
                    }
                }
                Err(Status::new(
                    Code::Internal,
                    format!(
                        "Unexpected result {} to method call hold_invoice_cancel",
                        result
                    ),
                ))
            }
        }
    }

    async fn hold_invoice_lookup(
        &self,
        request: tonic::Request<pb::HoldInvoiceLookupRequest>,
    ) -> Result<tonic::Response<pb::HoldInvoiceLookupResponse>, tonic::Status> {
        let req = request.into_inner();
        debug!("Client asked for Holdinvoicelookup");
        debug!("Holdinvoicelookup request: {:?}", req);
        let pay_hash = hex::encode(req.payment_hash.clone());
        debug!("payment_hash: {}", pay_hash);
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
                    format!("Unexpected result {} to method call hold_invoice_cancel", e),
                ));
            }
        };

        match result.get("code") {
            Some(_err) => Err(Status::new(
                Code::Internal,
                format!(
                    "Unexpected result {} to method call hold_invoice_cancel",
                    result
                ),
            )),
            None => {
                if let Some(state) = result.get("state") {
                    if let Ok(hs) = Holdstate::from_str(state.as_str().unwrap()) {
                        debug!("hs.as_i32:{} hs:{}", hs.as_i32(), hs);
                        let hisr = pb::HoldInvoiceLookupResponse {
                            state: hs.as_i32(),
                            htlc_expiry: if hs == Holdstate::Accepted {
                                Some(result.get("htlc_expiry").unwrap().as_u64().unwrap() as u32)
                            } else {
                                None
                            },
                        };
                        return Ok(tonic::Response::new(hisr));
                    }
                }
                Err(Status::new(
                    Code::Internal,
                    format!(
                        "Unexpected result {} to method call hold_invoice_cancel",
                        result
                    ),
                ))
            }
        }
    }
}
