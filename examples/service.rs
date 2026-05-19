use anyhow::Result;
use irpc::channel::none::{NoReceiver, NoSender};
use n0_schema::{schema, serialize_service};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MyService;

impl irpc::Service for MyService {
    type Message = v2::Proto;
}

mod v1 {

    use super::*;

    impl irpc::Channels<MyService> for GetRequest {
        type Rx = NoReceiver;
        type Tx = NoSender;
    }

    impl irpc::Channels<MyService> for PutRequest {
        type Rx = NoReceiver;
        type Tx = NoSender;
    }

    #[schema(Nominal)]
    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub struct GetRequest {
        pub key: String,
    }

    #[schema(Nominal)]
    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub struct PutRequest {
        pub key: String,
        pub value: String,
    }

    #[serialize_service(MyService)]
    #[derive(Debug, PartialEq, Eq)]
    pub enum Proto {
        Get(GetRequest),
        Put(PutRequest),
    }
}

mod v2 {

    impl irpc::Channels<MyService> for GetRequest {
        type Rx = NoReceiver;
        type Tx = NoSender;
    }

    impl irpc::Channels<MyService> for PutRequest {
        type Rx = NoReceiver;
        type Tx = NoSender;
    }

    use super::*;
    #[schema(Nominal)]
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct GetRequest {
        pub key: String,
    }

    #[schema(Nominal)]
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct PutRequest {
        pub key: String,
        pub value: Option<String>,
    }

    #[serialize_service(MyService)]
    #[derive(Debug, PartialEq, Eq)]
    pub enum Proto {
        Get(GetRequest),
        Put(PutRequest),
        V1Put(v1::PutRequest),
    }
}

fn roundtrip<T: Serialize, T2: DeserializeOwned>(
    value: T,
) -> std::result::Result<T2, postcard::Error> {
    let bytes = postcard::to_allocvec(&value)?;
    let value: T2 = postcard::from_bytes(&bytes)?;
    Ok(value)
}

fn main() -> Result<()> {
    {
        for (name, schema, hash) in v1::Proto::schemas() {
            println!(
                "{name}\n{}\n{}\n",
                hex::encode(hash),
                schema.pretty_print(0)
            );
        }
        let msg = v1::Proto::Get(v1::GetRequest {
            key: "key".to_string(),
        });
        let msg: v2::Proto = roundtrip(msg)?;
        println!("{:?}", msg);
    }
    {
        let msg = v1::Proto::Put(v1::PutRequest {
            key: "key".to_string(),
            value: "value".to_string(),
        });
        let msg: v2::Proto = roundtrip(msg)?;
        println!("{:?}", msg);
    }
    Ok(())
}
