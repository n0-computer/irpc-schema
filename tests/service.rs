use irpc::{channel::oneshot, rpc_requests};
use irpc_schema::{merge_services, serialize_service, HasSchema};
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

#[rpc_requests(message = CountMessage)]
#[serialize_service]
#[derive(Debug)]
enum CountProtocol {
    #[rpc(tx = oneshot::Sender<u64>)]
    #[wrap(Count, derive(HasSchema))]
    Count,
}

#[merge_services(message = MergedMessage, handler = MergedHandler)]
#[derive(Debug)]
enum MergedProtocol {
    Echo(EchoProtocol),
    Count(CountProtocol),
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

#[test]
fn test_merged_service_accepts_the_requests_of_each_service() -> TestResult<()> {
    let bytes = postcard::to_allocvec(&EchoProtocol::from(Echo("hi".to_string())))?;
    let value: MergedProtocol = postcard::from_bytes(&bytes)?;
    assert!(matches!(value, MergedProtocol::Echo(EchoProtocol::Echo(_))));
    let bytes = postcard::to_allocvec(&CountProtocol::from(Count))?;
    let value: MergedProtocol = postcard::from_bytes(&bytes)?;
    assert!(matches!(
        value,
        MergedProtocol::Count(CountProtocol::Count(_))
    ));
    Ok(())
}

#[test]
fn test_merged_service_lists_the_requests_of_each_service() {
    let names: Vec<_> = MergedProtocol::schemas().map(|(name, _, _)| name).collect();
    assert_eq!(names, ["Echo", "Add", "Count"]);
}

#[test]
fn test_merged_handler_converts_into_a_handler() {
    let (echo_tx, _echo_rx) = tokio::sync::mpsc::channel(1);
    let (count_tx, _count_rx) = tokio::sync::mpsc::channel(1);
    let handler = MergedHandler {
        echo: irpc::LocalSender::<EchoProtocol>::from(echo_tx).into(),
        count: irpc::LocalSender::<CountProtocol>::from(count_tx).into(),
    };
    let _: irpc::rpc::Handler<MergedProtocol> = handler.into();
}
