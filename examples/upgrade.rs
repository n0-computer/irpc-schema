//! Serves version 1 and version 2 clients of a store on one ALPN.
//!
//! Version 2 changes keys and values from `String` to `Bytes`, adds
//! `Delete`, and removes `Append`. `Len` does not change, so version 2
//! reuses the version 1 type, and both versions send the same hash for it.
//!
//! The store actor speaks only version 2. The version 1 handler converts
//! each version 1 request into a version 2 request and forwards it to the
//! actor, with the same channels. Only `Append` has no version 2 form, so
//! the version 1 handler builds it from version 2 requests.

use std::collections::BTreeMap;

use bytes::Bytes;
use iroh::{endpoint::presets, protocol::Router, Endpoint};
use irpc::{
    iroh::{IrohLazyRemoteConnection, IrohProtocol},
    rpc::{CloseConnection, Handler},
    Client, LocalSender, WithChannels,
};
use irpc_schema::merge_services;
use n0_future::task::AbortOnDropHandle;

/// The released version. It never changes.
mod v1 {
    use irpc::channel::oneshot;
    use irpc::rpc_requests;
    use irpc_schema::{serialize_service, HasSchema};

    #[rpc_requests(message = StoreMessage)]
    #[serialize_service]
    #[derive(Debug)]
    pub enum StoreProtocol {
        #[rpc(tx = oneshot::Sender<()>)]
        #[wrap(Set, derive(HasSchema))]
        Set { key: String, value: String },
        #[rpc(tx = oneshot::Sender<Option<String>>)]
        #[wrap(Get, derive(HasSchema))]
        Get { key: String },
        #[rpc(tx = oneshot::Sender<u64>)]
        #[wrap(Len, derive(HasSchema))]
        Len,
        #[rpc(tx = oneshot::Sender<()>)]
        #[wrap(Append, derive(HasSchema))]
        Append { key: String, value: String },
    }
}

mod v2 {
    use bytes::Bytes;
    use irpc::channel::oneshot;
    use irpc::rpc_requests;
    use irpc_schema::{serialize_service, HasSchema};

    #[rpc_requests(message = StoreMessage)]
    #[serialize_service]
    #[derive(Debug)]
    pub enum StoreProtocol {
        #[rpc(tx = oneshot::Sender<()>)]
        #[wrap(Set, derive(HasSchema))]
        Set { key: Bytes, value: Bytes },
        #[rpc(tx = oneshot::Sender<Option<Bytes>>)]
        #[wrap(Get, derive(HasSchema))]
        Get { key: Bytes },
        /// Unchanged from version 1, so it keeps its hash.
        #[rpc(tx = oneshot::Sender<u64>)]
        Len(super::v1::Len),
        /// Returns whether the key existed.
        #[rpc(tx = oneshot::Sender<bool>)]
        #[wrap(Delete, derive(HasSchema))]
        Delete { key: Bytes },
    }
}

/// Both versions. The first variant that knows a hash gets the request.
#[merge_services(message = StoreServerMessage, handler = StoreServerHandler)]
#[derive(Debug)]
enum StoreServer {
    V2(v2::StoreProtocol),
    V1(v1::StoreProtocol),
}

const ALPN: &[u8] = b"irpc-schema/example/upgrade";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (store, _store_task) = spawn_store();
    let server = Endpoint::bind(presets::Minimal).await?;
    let router = Router::builder(server.clone())
        .accept(ALPN, IrohProtocol::new(server_handler(store)))
        .spawn();

    let client = Endpoint::bind(presets::Minimal).await?;
    let conn = IrohLazyRemoteConnection::new(client.clone(), server.addr(), ALPN.to_vec());
    let old = Client::<v1::StoreProtocol>::boxed(conn.clone());
    let new = Client::<v2::StoreProtocol>::boxed(conn);

    let (key, value) = ("a".to_string(), "x".to_string());
    old.rpc(v1::Set { key, value }).await?;
    let (key, value) = ("a".to_string(), "yz".to_string());
    old.rpc(v1::Append { key, value }).await?;
    let (key, value) = (Bytes::from("b"), Bytes::from_static(&[0xff]));
    new.rpc(v2::Set { key, value }).await?;

    let key = Bytes::from("a");
    println!("v2 get a: {:?}", new.rpc(v2::Get { key }).await?);
    let key = "b".to_string();
    println!("v1 get b: {:?}", old.rpc(v1::Get { key }).await?);
    let key = Bytes::from("b");
    println!("v2 delete b: {}", new.rpc(v2::Delete { key }).await?);
    println!("v1 len: {}", old.rpc(v1::Len).await?);

    client.close().await;
    router.shutdown().await?;
    Ok(())
}

/// Spawns the store actor, which speaks version 2 only.
fn spawn_store() -> (LocalSender<v2::StoreProtocol>, AbortOnDropHandle<()>) {
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let task = tokio::spawn(async move {
        let mut items = BTreeMap::<Bytes, Bytes>::new();
        while let Some(msg) = rx.recv().await {
            match msg {
                v2::StoreMessage::Set(WithChannels { inner, tx, .. }) => {
                    items.insert(inner.key, inner.value);
                    tx.send(()).await.ok();
                }
                v2::StoreMessage::Get(WithChannels { inner, tx, .. }) => {
                    tx.send(items.get(&inner.key).cloned()).await.ok();
                }
                v2::StoreMessage::Len(WithChannels { tx, .. }) => {
                    tx.send(items.len() as u64).await.ok();
                }
                v2::StoreMessage::Delete(WithChannels { inner, tx, .. }) => {
                    tx.send(items.remove(&inner.key).is_some()).await.ok();
                }
            }
        }
    });
    (tx.into(), AbortOnDropHandle::new(task))
}

/// Returns a handler for both versions. Both end up at the store actor.
fn server_handler(store: LocalSender<v2::StoreProtocol>) -> StoreServerHandler {
    let v2 = Handler::from(store.clone());
    let v1 = Handler::<v1::StoreProtocol>::sequential(move |msg| {
        let store = store.clone();
        async move {
            forward_v1(&store, msg)
                .await
                .map_err(|_| CloseConnection::new(0, "store stopped"))
        }
    });
    StoreServerHandler { v2, v1 }
}

/// Converts a version 1 request into version 2 requests to the store.
async fn forward_v1(
    store: &LocalSender<v2::StoreProtocol>,
    msg: v1::StoreMessage,
) -> irpc::Result<()> {
    match msg {
        v1::StoreMessage::Set(WithChannels { inner, tx, .. }) => {
            let (key, value) = (inner.key.into(), inner.value.into());
            store.send((v2::Set { key, value }, tx)).await?;
        }
        v1::StoreMessage::Get(WithChannels { inner, tx, .. }) => {
            // A value that is not valid UTF-8 has no version 1 form.
            let tx = tx.with_map(|value: Option<Bytes>| value.and_then(utf8));
            store
                .send((
                    v2::Get {
                        key: inner.key.into(),
                    },
                    tx,
                ))
                .await?;
        }
        v1::StoreMessage::Len(WithChannels { inner, tx, .. }) => {
            // The merged protocol gives `Len` to version 2 first, so this only
            // runs if version 1 is served on its own.
            store.send((inner, tx)).await?;
        }
        v1::StoreMessage::Append(WithChannels { inner, tx, .. }) => {
            // Version 2 has no `Append`, so build it from `Get` and `Set`. This
            // is not atomic: a concurrent `Set` between the two can get lost.
            let client = Client::from(store.clone());
            let key = Bytes::from(inner.key);
            let old = client.rpc(v2::Get { key: key.clone() }).await?;
            let mut value = old.map(Vec::from).unwrap_or_default();
            value.extend_from_slice(inner.value.as_bytes());
            client
                .rpc(v2::Set {
                    key,
                    value: value.into(),
                })
                .await?;
            tx.send(()).await?;
        }
    }
    Ok(())
}

fn utf8(bytes: Bytes) -> Option<String> {
    String::from_utf8(bytes.into()).ok()
}
