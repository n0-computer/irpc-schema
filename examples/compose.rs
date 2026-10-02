//! Serves two independent services on one ALPN.
//!
//! `PingProtocol` and `KvProtocol` know nothing about each other.
//! `AppProtocol` merges both, so one handler serves both on one ALPN, and
//! plain clients of each service share one connection.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use iroh::{endpoint::presets, protocol::Router, Endpoint};
use irpc::{
    channel::oneshot,
    iroh::{IrohLazyRemoteConnection, IrohProtocol},
    rpc::Handler,
    rpc_requests, Client, WithChannels,
};
use irpc_schema::{merge_services, serialize_service, HasSchema};

#[rpc_requests(message = PingMessage)]
#[serialize_service]
#[derive(Debug)]
enum PingProtocol {
    #[rpc(tx = oneshot::Sender<u64>)]
    #[wrap(Ping, derive(HasSchema))]
    Ping(u64),
}

#[rpc_requests(message = KvMessage)]
#[serialize_service]
#[derive(Debug)]
enum KvProtocol {
    #[rpc(tx = oneshot::Sender<()>)]
    #[wrap(Set, derive(HasSchema))]
    Set { key: String, value: String },
    #[rpc(tx = oneshot::Sender<Option<String>>)]
    #[wrap(Get, derive(HasSchema))]
    Get { key: String },
}

/// Both services. Each request keeps the wire format of its own service.
#[merge_services(message = AppMessage, handler = AppHandler)]
#[derive(Debug)]
enum AppProtocol {
    Ping(PingProtocol),
    Kv(KvProtocol),
}

const ALPN: &[u8] = b"irpc-schema/example/compose";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server = Endpoint::bind(presets::Minimal).await?;
    let router = Router::builder(server.clone())
        .accept(
            ALPN,
            IrohProtocol::new(AppHandler {
                ping: ping_handler(),
                kv: kv_handler(),
            }),
        )
        .spawn();

    let client = Endpoint::bind(presets::Minimal).await?;
    let conn = IrohLazyRemoteConnection::new(client.clone(), server.addr(), ALPN.to_vec());
    let ping = Client::<PingProtocol>::boxed(conn.clone());
    let kv = Client::<KvProtocol>::boxed(conn);

    println!("ping: {}", ping.rpc(Ping(7)).await?);
    let (key, value) = ("hello".to_string(), "world".to_string());
    kv.rpc(Set { key, value }).await?;
    let key = "hello".to_string();
    println!("get: {:?}", kv.rpc(Get { key }).await?);

    client.close().await;
    router.shutdown().await?;
    Ok(())
}

fn ping_handler() -> Handler<PingProtocol> {
    Handler::sequential(|msg| async move {
        let PingMessage::Ping(WithChannels { inner, tx, .. }) = msg;
        tx.send(inner.0).await.ok();
        Ok(())
    })
}

fn kv_handler() -> Handler<KvProtocol> {
    let items = Arc::new(Mutex::new(BTreeMap::new()));
    Handler::sequential(move |msg| {
        let items = items.clone();
        async move {
            match msg {
                KvMessage::Set(WithChannels { inner, tx, .. }) => {
                    items
                        .lock()
                        .expect("poisoned")
                        .insert(inner.key, inner.value);
                    tx.send(()).await.ok();
                }
                KvMessage::Get(WithChannels { inner, tx, .. }) => {
                    let value = items.lock().expect("poisoned").get(&inner.key).cloned();
                    tx.send(value).await.ok();
                }
            }
            Ok(())
        }
    })
}
