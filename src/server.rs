use crate::hold::{hold_invoice, hold_invoice_cancel, hold_invoice_lookup, hold_invoice_settle};
use crate::model::{self, Holdstate, PluginState};
use crate::pb;
use crate::pb::hold_server::Hold;
use anyhow::Result;
use cln_plugin::Plugin;
use serde_json::{json, Map};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
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
        log::debug!("Client asked for Holdinvoice");
        log::trace!("Holdinvoice request: {:?}", req);
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
        log::debug!("{:?}", result);
        match serde_json::from_value::<model::HoldInvoiceResponse>(result.clone()) {
            Ok(r) => {
                log::trace!("Holdinvoice response: {:?}", r);
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
        log::debug!("Client asked for Holdinvoicesettle");
        log::debug!("Holdinvoicesettle request: {:?}", req);
        let mut req_map = Map::new();
        if let Some(p_h) = req.payment_hash {
            req_map.insert(
                "payment_hash".to_string(),
                serde_json::Value::String(hex::encode(p_h)),
            );
        }
        if let Some(p_i) = req.preimage {
            req_map.insert(
                "preimage".to_string(),
                serde_json::Value::String(hex::encode(p_i)),
            );
        }
        log::debug!("Holdinvoicesettle request encoded: {:?}", req_map);
        let result = match hold_invoice_settle(
            self.plugin.clone(),
            serde_json::Value::Object(req_map),
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
        log::debug!("Client asked for Holdinvoicecancel");
        log::debug!("Holdinvoicecancel request: {:?}", req);
        let pay_hash = hex::encode(req.payment_hash.clone());
        log::debug!("payment_hash: {}", pay_hash);
        let result = match hold_invoice_cancel(
            self.plugin.clone(),
            json!({"payment_hash":serde_json::Value::String(pay_hash)}),
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
        log::debug!("Client asked for Holdinvoicelookup");
        log::debug!("Holdinvoicelookup request: {:?}", req);
        let lookup_request = if let Some(ph) = req.payment_hash {
            json!({"payment_hash":serde_json::Value::String(hex::encode(ph))})
        } else {
            json!({})
        };
        log::debug!("lookup_request: {}", lookup_request);
        let result = match hold_invoice_lookup(self.plugin.clone(), lookup_request).await {
            Ok(res) => res,
            Err(e) => {
                return Err(Status::new(
                    Code::Internal,
                    format!("Unexpected result {} to method call hold_invoice_cancel", e),
                ));
            }
        };

        match serde_json::from_value::<model::HoldLookupResponse>(result) {
            Ok(lookup) => Ok(tonic::Response::new(lookup.into())),
            Err(e) => Err(Status::new(
                Code::Internal,
                format!("Could not deserialize HoldLookupResponse: {}", e),
            )),
        }
    }

    async fn hold_invoice_version(
        &self,
        _request: tonic::Request<pb::HoldInvoiceVersionRequest>,
    ) -> Result<tonic::Response<pb::HoldInvoiceVersionResponse>, tonic::Status> {
        Ok(tonic::Response::new(pb::HoldInvoiceVersionResponse {
            version: format!("v{}", env!("CARGO_PKG_VERSION")),
        }))
    }

    type SubscribeHoldInvoiceAcceptedStream = Box<
        dyn tokio_stream::Stream<Item = Result<pb::HoldInvoiceAcceptedNotification, Status>>
            + Send
            + Unpin
            + 'static,
    >;

    async fn subscribe_hold_invoice_accepted(
        &self,
        _request: tonic::Request<pb::HoldInvoiceAcceptedRequest>,
    ) -> Result<tonic::Response<Self::SubscribeHoldInvoiceAcceptedStream>, tonic::Status> {
        let receiver = self.plugin.state().notification.subscribe();
        let stream = BroadcastStream::new(receiver).map(|s| match s {
            Ok(notification) => Ok(notification.into()),
            Err(e) => Err(Status::internal(format!(
                "Notifications broadcast error: {e}"
            ))),
        });

        Ok(tonic::Response::new(Box::new(stream)))
    }
}
