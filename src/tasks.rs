use std::time::Duration;

use anyhow::Error;

use cln_plugin::Plugin;
use cln_rpc::model::requests::ListinvoicesRequest;
use cln_rpc::ClnRpc;
use log::info;
use tokio::time::{self, Instant};

use crate::model::PluginState;
use crate::rpc::{del_datastore_state, listdatastore_all};
use crate::util::make_rpc_path;

pub async fn autoclean_holdinvoice_db(plugin: Plugin<PluginState>) -> Result<(), Error> {
    time::sleep(Duration::from_secs(120)).await;
    info!("Starting autoclean_holdinvoice_db");

    let rpc_path = make_rpc_path(plugin.clone());
    loop {
        let now = Instant::now();
        let mut count = 0;
        {
            let mut rpc = ClnRpc::new(&rpc_path).await?;
            let node_invoices = rpc
                .call_typed(&ListinvoicesRequest {
                    index: None,
                    invstring: None,
                    label: None,
                    limit: None,
                    offer_id: None,
                    payment_hash: None,
                    start: None,
                })
                .await?
                .invoices;

            let payment_hashes: Vec<String> = node_invoices
                .iter()
                .map(|invoice| invoice.payment_hash.to_string())
                .collect();

            let datastore = listdatastore_all(&mut rpc).await?.datastore;
            for data in datastore {
                if !payment_hashes.contains(&data.key[1]) {
                    let _res = del_datastore_state(&mut rpc, data.key[1].clone()).await;
                    count += 1;
                }
            }
        }
        info!(
            "cleaned up {} holdinvoice database entries in {}ms",
            count,
            now.elapsed().as_millis()
        );
        time::sleep(Duration::from_secs(3_600)).await;
    }
}
