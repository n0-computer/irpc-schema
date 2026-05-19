use anyhow::Result;
use n0_schema::{schema, serialize_stable};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

mod v1 {
    use super::*;

    #[schema(Nominal(name = "v1::GetRequest"))]
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

    #[serialize_stable]
    #[derive(Debug, PartialEq, Eq)]
    pub enum Proto {
        Get(GetRequest),
        Put(PutRequest),
    }
}

mod v2 {
    use super::*;
    #[schema(Nominal(name = "v1::GetRequest"))]
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

    #[serialize_stable]
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
