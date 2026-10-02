use irpc::{channel::oneshot, rpc_requests};
use irpc_schema::{serialize_service, HasSchema};
use testresult::TestResult;

#[rpc_requests(message = EchoMessage)]
#[serialize_service]
#[derive(Debug)]
enum EchoProtocol {
    #[rpc(tx = oneshot::Sender<String>)]
    #[wrap(Echo, derive(HasSchema))]
    Echo(String),
    #[rpc(tx = oneshot::Sender<u64>)]
    #[wrap(Add, derive(HasSchema))]
    Add { a: u64, b: u64 },
}

#[test]
fn test_service_tags_requests_with_schema_hashes() -> TestResult<()> {
    let bytes = postcard::to_allocvec(&EchoProtocol::from(Echo("hi".to_string())))?;
    let (_, _, hash) = EchoProtocol::schemas()
        .find(|(name, _, _)| *name == "Echo")
        .expect("Echo is listed");
    assert_eq!(bytes[..32], hash);
    let value: EchoProtocol = postcard::from_bytes(&bytes)?;
    assert!(matches!(value, EchoProtocol::Echo(Echo(text)) if text == "hi"));
    Ok(())
}

#[tokio::test]
async fn test_service_works_with_a_local_client() -> TestResult<()> {
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let client = irpc::Client::<EchoProtocol>::local(tx);
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            match msg {
                EchoMessage::Echo(msg) => msg.tx.send(msg.inner.0.clone()).await.ok(),
                EchoMessage::Add(msg) => msg.tx.send(msg.inner.a + msg.inner.b).await.ok(),
            };
        }
    });
    assert_eq!(client.rpc(Echo("hi".to_string())).await?, "hi");
    assert_eq!(client.rpc(Add { a: 1, b: 2 }).await?, 3);
    Ok(())
}
