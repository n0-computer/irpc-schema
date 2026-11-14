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

/// The meta protocol is how peers find out about each other, so its hash must never change.
#[test]
fn list_cases_hash_is_stable() {
    assert_eq!(
        hex::encode(request_hash::<MetaProtocol, ListCases>()),
        "1638de147d69bebaaad557ce149756c493679f43144664bd886e990bb91b9968"
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

    // A later client checks before it sends.
    let supported = |hash: [u8; 32]| cases.iter().any(|case| case.hash == hash);
    assert!(supported(request_hash::<LaterStoreProtocol, Get>()));
    assert!(!supported(request_hash::<LaterStoreProtocol, Delete>()));
    // The check agrees with the server.
    let later = Client::<LaterStoreProtocol>::boxed(connection.clone());
    assert_eq!(later.rpc(Get { key: "k".into() }).await?, None);
    Ok(())
}
