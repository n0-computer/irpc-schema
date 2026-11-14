//! Serves three irpc services on one endpoint, each with its own actor.
//!
//! Each client only knows its own service. Since n0-schema identifies each
//! request by the schema hash of its type, the server can tell the requests
//! of the services apart without any change to their wire format.
use anyhow::Result;
use irpc::{channel::oneshot, rpc_requests, Client};
use n0_schema::{combine_services, schema, serialize_service};
use serde::{Deserialize, Serialize};

mod store {
    use super::*;

    #[schema(Nominal)]
    #[derive(Debug, Serialize, Deserialize)]
    pub struct Get {
        pub key: String,
    }

    #[schema(Nominal)]
    #[derive(Debug, Serialize, Deserialize)]
    pub struct Put {
        pub key: String,
        pub value: String,
    }

    #[rpc_requests(message = StoreMessage)]
    #[serialize_service(StoreProtocol)]
    #[derive(Debug)]
    pub enum StoreProtocol {
        #[rpc(tx = oneshot::Sender<Option<String>>)]
        Get(Get),
        #[rpc(tx = oneshot::Sender<()>)]
        Put(Put),
    }

    pub async fn actor(mut rx: tokio::sync::mpsc::Receiver<StoreMessage>) {
        let mut map = std::collections::BTreeMap::new();
        while let Some(msg) = rx.recv().await {
            match msg {
                StoreMessage::Get(msg) => {
                    let value = map.get(&msg.key).cloned();
                    msg.tx.send(value).await.ok()
                }
                StoreMessage::Put(msg) => {
                    let irpc::WithChannels { inner, tx, .. } = msg;
                    map.insert(inner.key, inner.value);
                    tx.send(()).await.ok()
                }
            };
        }
    }
}

mod counter {
    use super::*;

    #[schema(Nominal)]
    #[derive(Debug, Serialize, Deserialize)]
    pub struct Increment {}

    #[rpc_requests(message = CounterMessage)]
    #[serialize_service(CounterProtocol)]
    #[derive(Debug)]
    pub enum CounterProtocol {
        #[rpc(tx = oneshot::Sender<u64>)]
        Increment(Increment),
    }

    pub async fn actor(mut rx: tokio::sync::mpsc::Receiver<CounterMessage>) {
        let mut count = 0;
        while let Some(CounterMessage::Increment(msg)) = rx.recv().await {
            count += 1;
            msg.tx.send(count).await.ok();
        }
    }
}

mod echo {
    use super::*;

    #[schema(Nominal)]
    #[derive(Debug, Serialize, Deserialize)]
    pub struct Echo {
        pub text: String,
    }

    #[rpc_requests(message = EchoMessage)]
    #[serialize_service(EchoProtocol)]
    #[derive(Debug)]
    pub enum EchoProtocol {
        #[rpc(tx = oneshot::Sender<String>)]
        Echo(Echo),
    }

    pub async fn actor(mut rx: tokio::sync::mpsc::Receiver<EchoMessage>) {
        while let Some(EchoMessage::Echo(msg)) = rx.recv().await {
            let irpc::WithChannels { inner, tx, .. } = msg;
            tx.send(inner.text).await.ok();
        }
    }
}

use counter::CounterProtocol;
use echo::EchoProtocol;
use store::StoreProtocol;

/// What the server reads: a request of any of the three services.
#[combine_services]
#[derive(Debug)]
enum Combined {
    Store(StoreProtocol),
    Counter(CounterProtocol),
    Echo(EchoProtocol),
}

/// A client that knows a request the server does not.
mod later {
    use super::*;

    #[schema(Nominal)]
    #[derive(Debug, Serialize, Deserialize)]
    pub struct Echo {
        pub text: String,
        pub times: u32,
    }

    #[rpc_requests(message = LaterMessage)]
    #[serialize_service(LaterProtocol)]
    #[derive(Debug)]
    pub enum LaterProtocol {
        #[rpc(tx = oneshot::Sender<String>)]
        Echo(Echo),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    for (name, _, hash) in Combined::schemas() {
        println!("{name}: {}", hex::encode(&hash[..8]));
    }

    let (store_tx, store_rx) = tokio::sync::mpsc::channel(16);
    let (counter_tx, counter_rx) = tokio::sync::mpsc::channel(16);
    let (echo_tx, echo_rx) = tokio::sync::mpsc::channel(16);
    tokio::spawn(store::actor(store_rx));
    tokio::spawn(counter::actor(counter_rx));
    tokio::spawn(echo::actor(echo_rx));

    let (server, cert) = irpc::noq::make_server_endpoint("127.0.0.1:0".parse()?)?;
    let addr = server.local_addr()?;
    let handler = Combined::handler(store_tx, counter_tx, echo_tx).skip_bad_requests(true);
    tokio::spawn(irpc::noq::listen(server, handler));

    let endpoint = irpc::noq::make_client_endpoint("127.0.0.1:0".parse()?, &[&cert])?;
    let store = Client::<StoreProtocol>::noq(endpoint.clone(), addr);
    let counter = Client::<CounterProtocol>::noq(endpoint.clone(), addr);
    let echo = Client::<EchoProtocol>::noq(endpoint.clone(), addr);

    store
        .rpc(store::Put {
            key: "hello".into(),
            value: "world".into(),
        })
        .await?;
    let value = store
        .rpc(store::Get {
            key: "hello".into(),
        })
        .await?;
    println!("store: {value:?}");
    assert_eq!(value.as_deref(), Some("world"));

    counter.rpc(counter::Increment {}).await?;
    let count = counter.rpc(counter::Increment {}).await?;
    println!("counter: {count}");
    assert_eq!(count, 2);

    let text = echo.rpc(echo::Echo { text: "hi".into() }).await?;
    println!("echo: {text}");
    assert_eq!(text, "hi");

    // A request that no combined service knows fails alone.
    let later = Client::<later::LaterProtocol>::noq(endpoint.clone(), addr);
    let err = later
        .rpc(later::Echo {
            text: "hi".into(),
            times: 2,
        })
        .await
        .expect_err("unknown request");
    println!("later: {err}");
    assert_eq!(
        err.remote_code(),
        Some(irpc::rpc::ErrorCode::RequestDecodeFailed)
    );
    Ok(())
}
