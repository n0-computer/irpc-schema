//! Lists the requests that an irpc service on iroh supports.
//!
//! The service must include `n0_schema::meta::MetaProtocol`. To try it, start
//! the demo service in one terminal, and list its requests in another:
//!
//! ```sh
//! cargo run --example meta --features meta -- serve
//! cargo run --example meta --features meta -- list <endpoint-id> n0-schema/example/0
//! ```
use std::time::Duration;

use anyhow::{bail, Context, Result};
use iroh::{endpoint::presets, protocol::Router, Endpoint, EndpointId};
use irpc::{channel::oneshot, rpc_requests, Client, WithChannels};
use n0_schema::{
    combine_services,
    meta::{ListCases, MetaProtocol},
    schema, serialize_service,
};
use serde::{Deserialize, Serialize};

const DEMO_ALPN: &str = "n0-schema/example/0";

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["serve"] => serve().await,
        ["list", endpoint_id, alpn] => list(endpoint_id.parse()?, alpn).await,
        _ => bail!("usage: meta serve | meta list <endpoint-id> <alpn>"),
    }
}

/// Prints name, hash and schema of each request of the service.
async fn list(endpoint_id: EndpointId, alpn: &str) -> Result<()> {
    let endpoint = Endpoint::builder(presets::N0).bind().await?;
    let client = Client::<MetaProtocol>::iroh(endpoint.clone(), endpoint_id, alpn);
    let cases = tokio::time::timeout(Duration::from_secs(30), client.rpc(ListCases {}))
        .await
        .context("timed out")?
        .context("the service does not answer meta requests")?;
    for case in cases {
        println!(
            "{} {}: {}",
            hex::encode(&case.hash[..8]),
            case.name,
            case.schema
        );
    }
    endpoint.close().await;
    Ok(())
}

#[schema(Nominal)]
#[derive(Debug, Serialize, Deserialize)]
struct Get {
    key: String,
}

#[schema(Nominal)]
#[derive(Debug, Serialize, Deserialize)]
struct Put {
    key: String,
    value: String,
}

#[rpc_requests(message = StoreMessage)]
#[serialize_service(StoreProtocol)]
#[derive(Debug)]
enum StoreProtocol {
    #[rpc(tx = oneshot::Sender<Option<String>>)]
    Get(Get),
    #[rpc(tx = oneshot::Sender<()>)]
    Put(Put),
}

/// The demo service: a key-value store, plus the meta protocol.
#[combine_services]
#[derive(Debug)]
enum Demo {
    Meta(MetaProtocol),
    Store(StoreProtocol),
}

async fn serve() -> Result<()> {
    let (store_tx, mut store_rx) = tokio::sync::mpsc::channel::<StoreMessage>(16);
    tokio::spawn(async move {
        let mut map = std::collections::BTreeMap::new();
        while let Some(msg) = store_rx.recv().await {
            match msg {
                StoreMessage::Get(msg) => {
                    let value = map.get(&msg.key).cloned();
                    msg.tx.send(value).await.ok();
                }
                StoreMessage::Put(msg) => {
                    let WithChannels { inner, tx, .. } = msg;
                    map.insert(inner.key, inner.value);
                    tx.send(()).await.ok();
                }
            }
        }
    });
    let endpoint = Endpoint::builder(presets::N0).bind().await?;
    let handler = Demo::handler(n0_schema::meta::serve::<Demo>(), store_tx).skip_bad_requests(true);
    let router = Router::builder(endpoint.clone())
        .accept(DEMO_ALPN, irpc::iroh::IrohProtocol::new(handler))
        .spawn();
    println!("serving on {DEMO_ALPN}, list it with:");
    println!(
        "  cargo run --example meta --features meta -- list {} {DEMO_ALPN}",
        endpoint.id()
    );
    tokio::signal::ctrl_c().await?;
    router.shutdown().await?;
    Ok(())
}
