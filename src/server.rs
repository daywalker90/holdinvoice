use crate::hold::{hold_invoice, hold_invoice_cancel, hold_invoice_lookup, hold_invoice_settle};
use crate::model::{self, Holdstate, PluginState};
use crate::pb;
use crate::pb::hold_server::Hold;
use crate::util::u64_to_scid;
use anyhow::Result;
use bitcoin::hashes::Hash;
use bitcoin::secp256k1::PublicKey;
use cln_plugin::Plugin;
use cln_rpc::primitives::{Amount, Routehint, Routehop};
use lightning_invoice::{Bolt11Invoice, Bolt11InvoiceDescription, SignedRawBolt11Invoice};
use log::{debug, trace};
use std::collections::HashMap;
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
    async fn decode_bolt11(
        &self,
        request: tonic::Request<pb::DecodeBolt11Request>,
    ) -> Result<tonic::Response<pb::DecodeBolt11Response>, tonic::Status> {
        let req = request.into_inner();
        debug!("Client asked for decode_bolt11");
        trace!("decode_bolt11 request: {:?}", req);
        let raw_invoice =
            match SignedRawBolt11Invoice::from_str(&req.bolt11).map_err(|e| e.to_string()) {
                Ok(b11) => b11,
                Err(e) => {
                    return Err(Status::new(
                        Code::Internal,
                        format!(
                            "Invalid bolt11 string in method call decode_bolt11: {:?}",
                            e
                        ),
                    ))
                }
            };
        let invoice = match Bolt11Invoice::from_signed(raw_invoice) {
            Ok(iv) => iv,
            Err(e) => {
                return Err(Status::new(
                    Code::Internal,
                    format!("Invalid invoice in method call decode_bolt11: {:?}", e),
                ))
            }
        };
        let amount_msat = invoice
            .amount_milli_satoshis()
            .map(|amt| Amount::from_msat(amt).into());
        let mut description = None;
        let mut description_hash = None;
        match invoice.description() {
            Bolt11InvoiceDescription::Direct(desc) => {
                description = Some(desc.to_string());
            }
            Bolt11InvoiceDescription::Hash(hash) => {
                description_hash = Some(hash.0.to_byte_array().to_vec());
            }
        }

        let mut pb_route_hints = Vec::new();

        for hint in &invoice.route_hints() {
            let mut scid_map = HashMap::new();
            for hop in &hint.0 {
                match u64_to_scid(hop.short_channel_id) {
                    Ok(o) => scid_map.insert(hop.short_channel_id, o),
                    Err(e) => {
                        return Err(Status::new(
                            Code::InvalidArgument,
                            format!("Error parsing short channel id: {:?}", e),
                        ))
                    }
                };
            }

            let pb_route_hops = hint
                .0
                .iter()
                .map(|hop| {
                    let scid = scid_map.get(&hop.short_channel_id).unwrap();
                    Routehop {
                        id: PublicKey::from_str(&hop.src_node_id.to_string()).unwrap(),
                        scid: *scid,
                        feebase: Amount::from_msat(hop.fees.base_msat as u64),
                        feeprop: hop.fees.proportional_millionths,
                        expirydelta: hop.cltv_expiry_delta,
                    }
                })
                .collect();

            pb_route_hints.push(
                Routehint {
                    hops: pb_route_hops,
                }
                .into(),
            );
        }

        Ok(tonic::Response::new(pb::DecodeBolt11Response {
            description,
            description_hash,
            payment_hash: invoice.payment_hash().to_byte_array().to_vec(),
            expiry: invoice.expiry_time().as_secs(),
            amount_msat,
            route_hints: Some(pb::RoutehintList {
                hints: pb_route_hints,
            }),
            timestamp: invoice.duration_since_epoch().as_secs() as u32,
        }))
    }
}
