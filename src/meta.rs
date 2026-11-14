//! A protocol to ask a server which requests it supports.
//!
//! Add [`MetaProtocol`] to the services of a server with `combine_services`,
//! and answer it with [`serve`]:
//!
//! ```ignore
//! #[combine_services]
//! #[derive(Debug)]
//! enum Server {
//!     Meta(n0_schema::meta::MetaProtocol),
//!     Store(StoreProtocol),
//! }
//!
//! let handler = Server::handler(n0_schema::meta::serve::<Server>(), store_tx);
//! ```
//!
//! A client then lists the cases of the server with [`ListCases`], and checks
//! whether a request is among them with [`request_hash`](crate::request_hash).
//!
//! This protocol is how peers find out about each other's versions, so its
//! requests never change. New requests may be added.
use irpc::{channel::oneshot, rpc_requests, LocalSender, WithChannels};
use serde::{Deserialize, Serialize};

use crate::{schema, serialize_service, StableEnum};

/// Lists the cases that a server supports.
///
/// The schema names of the meta types have a prefix, so that no other
/// protocol has a request with the same schema by accident.
#[schema(Nominal(name = "n0_schema::meta::ListCases"))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListCases {}

/// One case that a server supports.
#[schema(Nominal(name = "n0_schema::meta::CaseInfo"))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaseInfo {
    /// The name of the variant on the server, for people.
    pub name: String,
    /// The schema hash that identifies the case on the wire.
    #[schema(atom = "blake3::Hash")]
    pub hash: [u8; 32],
    /// The schema in its text form, for people.
    pub schema: String,
}

/// Requests about the protocol of a server.
#[rpc_requests(message = MetaMessage, rpc_feature = "meta")]
#[serialize_service(MetaProtocol)]
#[derive(Debug)]
pub enum MetaProtocol {
    #[rpc(tx = oneshot::Sender<Vec<CaseInfo>>)]
    ListCases(ListCases),
}

/// Returns the cases of `T`, for [`ListCases`].
pub fn cases<T: StableEnum>() -> Vec<CaseInfo> {
    T::case_schemas()
        .into_iter()
        .map(|(name, schema, hash)| CaseInfo {
            name: name.to_owned(),
            hash,
            schema: schema.to_string(),
        })
        .collect()
}

/// Spawns a task that answers [`MetaProtocol`] requests about `T`, and returns its sender.
///
/// `T` is usually the union of all services of the server, including
/// [`MetaProtocol`] itself. This must be called inside a tokio runtime.
pub fn serve<T: StableEnum>() -> LocalSender<MetaProtocol> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<MetaMessage>(16);
    tokio::spawn(async move {
        let cases = cases::<T>();
        while let Some(msg) = rx.recv().await {
            match msg {
                MetaMessage::ListCases(msg) => {
                    let WithChannels { tx, .. } = msg;
                    tx.send(cases.clone()).await.ok();
                }
            }
        }
    });
    LocalSender::from(tx)
}
