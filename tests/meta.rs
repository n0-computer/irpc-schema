//! Listing the cases of a server with the meta protocol.
use irpc::{channel::oneshot, rpc_requests, Client, WithChannels};
use n0_schema::{
    combine_services,
    meta::{ListCases, MetaProtocol},
    request_hash, schema, serialize_service,
};
use serde::{Deserialize, Serialize};

#[schema(Nominal)]
#[derive(Debug, Serialize, Deserialize)]
struct Get {
    key: String,
}

#[schema(Nominal)]
#[derive(Debug, Serialize, Deserialize)]
struct Delete {
    key: String,
}

#[rpc_requests(message = StoreMessage)]
#[serialize_service(StoreProtocol)]
#[derive(Debug)]
enum StoreProtocol {
    /// Reads the value of a key.
    #[rpc(tx = oneshot::Sender<Option<String>>)]
    Get(Get),
}

/// A later version of the store, with a request the server does not know.
#[rpc_requests(message = LaterStoreMessage)]
#[serialize_service(LaterStoreProtocol)]
#[derive(Debug)]
enum LaterStoreProtocol {
    #[rpc(tx = oneshot::Sender<Option<String>>)]
    Get(Get),
    #[rpc(tx = oneshot::Sender<()>)]
    Delete(Delete),
}

#[combine_services]
#[derive(Debug)]
enum Server {
    Meta(MetaProtocol),
    Store(StoreProtocol),
}

/// Pin the meta protocol's hash for the uniform-product schema format.
/// Changes within this format must preserve discovery compatibility.
#[test]
fn list_cases_hash_is_stable() {
    assert_eq!(
        hex::encode(request_hash::<MetaProtocol, ListCases>()),
        "33209d7bf5c4b23a85ca001afd3e096291cfdee98add8dcaf3baf877c2dc5bad"
    );
}

#[tokio::test]
async fn client_finds_out_which_requests_the_server_knows() -> testresult::TestResult<()> {
    let (store_tx, mut store_rx) = tokio::sync::mpsc::channel::<StoreMessage>(4);
    tokio::spawn(async move {
        while let Some(StoreMessage::Get(msg)) = store_rx.recv().await {
            let WithChannels { tx, .. } = msg;
            tx.send(None).await.ok();
        }
    });
    let (server, cert) = irpc::noq::make_server_endpoint("127.0.0.1:0".parse()?)?;
    let addr = server.local_addr()?;
    let handler = Server::handler(n0_schema::meta::serve::<Server>(), store_tx);
    tokio::spawn(irpc::noq::listen(server, handler));

    let endpoint = irpc::noq::make_client_endpoint("127.0.0.1:0".parse()?, &[&cert])?;
    let connection = endpoint.connect(addr, "localhost")?.await?;
    let meta = Client::<MetaProtocol>::boxed(connection.clone());
    let cases = meta.rpc(ListCases {}).await?;
    let names: Vec<_> = cases.iter().map(|case| case.name.as_str()).collect();
    assert_eq!(names, ["ListCases", "Get"]);
    assert!(cases[1].schema.contains("\"key\""), "{}", cases[1].schema);
    assert_eq!(cases[1].docs, "Reads the value of a key.");
    assert!(cases[0].docs.starts_with("Lists"), "{}", cases[0].docs);

    // A later client checks before it sends.
    let supported = |hash: [u8; 32]| cases.iter().any(|case| case.hash == hash);
    assert!(supported(request_hash::<LaterStoreProtocol, Get>()));
    assert!(!supported(request_hash::<LaterStoreProtocol, Delete>()));
    // The check agrees with the server.
    let later = Client::<LaterStoreProtocol>::boxed(connection.clone());
    assert_eq!(later.rpc(Get { key: "k".into() }).await?, None);
    Ok(())
}
