# RFD: irpc 1.0 and irpc-schema

Status: draft. Date: 2026-10-01.

This RFD merges four documents into one proposal:

- `plans/changes-for-1.0.md` (this repo): the first RFD.
- `irpc/plans/changes-for-1.0.md`: the irpc 1.0 plan, items A to G and
  O1 to O8. Ids like `C1` refer to it.
- `irpc/plans/dealing-with-reconnects.md`: the reconnect plan.
- `plans/survey.md` and `plans/survey-evaluation.md` (main worktree):
  how 15 projects use irpc, and the gaps.

The source documents keep the reasoning. This one says what to build.

Status tags:

| Tag | Meaning |
|---|---|
| done | merged on main |
| branch | implemented on a branch (named) |
| planned | decided, no code |
| open | needs a decision |
| 1.x | additive, can land after 1.0 |

Code uses today's names (`WithChannels`, `Service`). Section 1.8 lists
the renames.

## Crates

| Crate | Version | Depends on | Contents |
|---|---|---|---|
| `irpc` | 1.0 | `irpc-derive`, noq, iroh (feature) | Core, channels, `irpc::rpc` (shared server code), `irpc::iroh`, `irpc::noq`, the macro |
| `irpc-derive` | 1.0 | | The `rpc_requests` macro (rename open) |
| `irpc-schema-core` (name open) | 0.x | serde, postcard, blake3 | `Schema`, `HasSchema`, the derive, stable enums, hash locks |
| `irpc-schema` | 0.x | `irpc` 1, `irpc-schema-core` | Schema-tagged services, merged services, versioning helpers |
| `irpc-iroh` | 0.18, last | `irpc` | `pub use irpc::iroh::*;` with a deprecation note (open: A1.5) |

irpc does not depend on the schema crates.

# Part 1: irpc 1.0

## 1.1 Define a protocol

```rust
use irpc::channel::{mpsc, oneshot};

#[irpc::rpc_requests(message = StoreMessage)]
#[derive(Debug, Serialize, Deserialize)]
pub enum StoreProtocol {
    #[rpc(tx = oneshot::Sender<Result<(), StoreError>>)]
    Auth(Auth),
    #[rpc(tx = oneshot::Sender<Option<String>>)]
    Get(Get),
    #[rpc(tx = mpsc::Sender<(String, String)>)]
    List(List),
    #[rpc(tx = oneshot::Sender<u64>)]
    #[wrap(Count)]
    Count,
    #[cfg(feature = "admin")]
    #[rpc(tx = oneshot::Sender<()>)]
    Wipe(Wipe),
}
```

- The macro keeps its name, or becomes `rpc` or `rpc_service`. The old
  name stays as a deprecated alias. *open (B1)*
- The output names irpc only through `irpc::__macro_exports`, so the
  user crate needs no `serde` or `tracing` dependency. *done (B2.1)*
- `crate = some::path` replaces `::irpc` in the output, for crates
  that re-export irpc, like irpc-schema. *planned (B2.2)*
- `#[cfg]` on a variant applies to every item the macro generates for
  it. *planned*
- A single-field tuple `#[wrap(PreVote)] PreVote(VoteRequest)` also
  gets `From<VoteRequest> for PreVote`. Use it when two variants carry
  the same payload type. *planned*
- `StoreProtocol::name(&self) -> &'static str` returns the variant
  name, for logs. *planned*
- The macro fixes alias visibility, alias docs, and the docs on the
  message enum. *planned (B3)*

## 1.2 Serve it

Forward every request to an actor:

```rust
let (tx, rx) = tokio::sync::mpsc::channel(16);
tokio::spawn(store_actor(rx));
let router = Router::builder(endpoint)
    .accept(STORE_ALPN, IrohProtocol::new(LocalSender::from(tx)))
    .spawn();
```

Or handle requests in a function:

```rust
let handler = Handler::<StoreProtocol>::sequential(|msg| async move {
    match msg {
        StoreMessage::Get(WithChannels { inner, tx, .. }) => {
            tx.send(lookup(&inner.key)).await.ok();
        }
        StoreMessage::Auth(_) => {
            return Err(CloseConnection::new(400, "unexpected auth"));
        }
        ..
    }
    Ok(())
});
```

`Handler<S>` *branch (`Frando/feat-handler`, C2, C5, C6)*:

| Constructor | Runs |
|---|---|
| `Handler::from_sender(local)`, `From<LocalSender<S>>` | sends each message to an actor, with backpressure |
| `Handler::sequential(f)` | `f(S::Message)`, one request at a time per connection |
| `Handler::concurrent(n, f)` | `f(S::Message)` in tasks, at most `n` per connection |
| `Handler::raw(f)` | `f(S, rx, tx)`, before irpc attaches the channels |
| `handler.call(req, rx, tx)` | another handler on one request, from inside `raw` |

All functions return `Result<(), CloseConnection>`. `Err` closes the
connection with that code and reason. `IrohProtocol::new`,
`Iroh0RttProtocol::new`, `listen`, and `handle_connection` take
`impl Into<Handler<S>>`.

Removed: `RemoteService::remote_handler`, `IrohProtocol::with_sender`.
*branch.* Proposed: also drop the shortcuts `IrohProtocol::sequential`
and `IrohProtocol::concurrent` for 1.0. *open*

### Per connection: auth first, state, remote id

Implement `ProtocolHandler`, read the first request, then serve the
rest with a handler built for this connection:

```rust
impl ProtocolHandler for StoreServer {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let Some(StoreMessage::Auth(auth)) = read_request::<StoreProtocol>(&conn).await? else {
            conn.close(401u32.into(), b"expected auth");
            return Ok(());
        };
        let caps = self.verify(conn.remote_id(), &auth.inner)?;
        auth.tx.send(Ok(())).await.ok();

        let session = Session::new(self.clone(), conn.remote_id(), caps);
        let handler = Handler::<StoreProtocol>::concurrent(16, move |msg| {
            let session = session.clone();
            async move { session.handle(msg).await }
        });
        handle_connection(&conn, handler).await?;
        session.closed(); // disconnect hook
        Ok(())
    }
}
```

This is the 1.0 answer to "handlers cannot see the connection". It
works today on `Frando/feat-handler` (`examples/auth.rs`).
`Handler::per_connection`, built on `AsyncFnMut` so that state needs no
lock, is *1.x*.

For `&mut` state with no lock, write the loop yourself:

```rust
while let Some(msg) = read_request::<StoreProtocol>(&conn).await? {
    session.handle(msg).await?;
}
```

`read_request` stays public. `read_request_raw` goes away: `Handler::raw`
covers it. *planned*

`IncomingRemoteConnection` becomes sealed and gets a typed remote id
(`SocketAddr` for noq, `EndpointId` for iroh) in place of
`remote_label() -> Option<String>`. *planned (A3.4, O8)*

## 1.3 Call it

```rust
let client = Client::<StoreProtocol>::iroh(endpoint, addr, STORE_ALPN); // branch (E14)
let local = Client::<StoreProtocol>::local(tx);
let value = client.rpc(Get { key }).await?;
let mut items = client.server_streaming(List, 16).await?;
```

Share one connection between clients of several protocols:

```rust
let conn = IrohLazyRemoteConnection::new(endpoint, addr, APP_ALPN.to_vec());
let store = Client::<StoreProtocol>::remote(conn.clone());
let ping = Client::<PingProtocol>::remote(conn);
```

Re-authenticate after a reconnect. The hook runs on every new
connection, under the connection lock, before any request:
*planned (reconnect plan P2/H1, needs the seal below)*

```rust
let client = Client::<StoreProtocol>::iroh(endpoint, addr, STORE_ALPN)
    .on_connect(move |client| {
        let token = token.clone();
        async move { client.rpc(Auth { token }).await?.map_err(AnyError::from) }
    });
```

- If the hook fails, irpc closes the connection, and the triggering
  request fails with the hook error. The next request connects again.
- `IrohLazyRemoteConnection::connect()` connects eagerly, so a bad
  token fails at startup. *planned*
- `RemoteConnection` becomes sealed (O4). The pool use case of dht2 and
  gossip2 moves to `LazyConnection::from_fn(connect_fn)`. *open, 1.x
  for `from_fn`*

Deadlines *1.x, error case planned*:

```rust
let client = client.with_timeout(Duration::from_secs(10));
```

## 1.4 Errors

All public error enums are `#[non_exhaustive]`. *planned (D1).* This
lands before every item below that adds an error case.

New cases and helpers:

```rust
match client.rpc(Get { key }).await {
    Ok(value) => ..,
    Err(err) if err.is_unsupported() => ..,      // the server does not know the request
    Err(err) if err.is_connection_lost() => ..,  // a new connection may work
    Err(err) if err.is_timeout() => ..,          // with_timeout
    Err(err) => ..,
}

// response type Result<T, MyError>, with MyError: From<irpc::Error>
let value = client.rpc(Get { key }).await.flatten_remote()?;
```

| Item | Status |
|---|---|
| `is_connection_lost`, `is_unsupported`, `is_too_large` | planned |
| `is_timeout` and the timeout case | case planned, `with_timeout` 1.x |
| `flatten_remote` (trait on `irpc::Result<Result<T, E>>`) | 1.x |
| `read_request` returns `ReadRequestError { InvalidRequest, Connection }` | planned (open question 2 of the irpc plan) |

## 1.5 Wire behavior

Request: a bi stream, then `varint(len) || postcard(S)`, or
`postcard((Option<SpanContextCarrier>, S))` with span propagation. The
crate docs specify this as a contract. *planned (G1)*

- A request that does not decode: the server resets that stream with
  `ERROR_CODE_INVALID_REQUEST = 3`, logs at `warn`, and accepts the
  next stream. The client gets `is_unsupported()`. *planned (C1)*
- A request larger than the limit: the same, with code 1. *planned
  (C1.3)*
- `read_request` skips nothing: it resets the stream and returns
  `ReadRequestError::InvalidRequest`. The caller decides. *planned*
- Codes 0 to 255 belong to irpc. Applications use codes above.
  *planned (C1.1)*

Streams:

```rust
// server
tx.abort(MY_CODE);          // reset the stream with a code
// a sender dropped during a panic resets with an irpc code
// client
match rx.recv().await {
    Ok(Some(item)) => ..,
    Ok(None) => ..,                                    // the sender finished
    Err(err) if err.aborted_with() == Some(MY_CODE) => ..,
    Err(err) => ..,
}
```

*planned: the error case and the reset on panic. 1.x: `abort`.* This
also covers P4 of the reconnect plan (refusal as a reset code).
`into_stream` ends after the first error. *planned*

## 1.6 Modules and features

| Path | Contents | Status |
|---|---|---|
| `irpc::rpc` | `Handler`, `CloseConnection`, `read_request`, `handle_connection`, `IncomingRemoteConnection`, `RemoteConnection`, constants | branch (A3) |
| `irpc::iroh` | `IrohProtocol`, `Iroh0RttProtocol`, `IrohLazyRemoteConnection`, `listen` | branch (A1) |
| `irpc::noq` | the noq lazy connection, `listen`, endpoint helpers | branch (A2) |
| `irpc::__macro_exports` | hidden re-exports for the macro | done (B2.1) |

Features: `default = ["rpc", "iroh", "spans", "stream", "derive"]`.
`noq_endpoint_setup` (renamed `noq-endpoint-setup`, A4.2) is opt-in.
*branch, rename planned.* No `cdylib` (A5). *planned*

## 1.7 API freeze

*planned unless noted*

- Channel types (`mpsc::Sender`, `mpsc::Receiver`, `oneshot::Sender`,
  `oneshot::Receiver`) are opaque structs. `From`/`TryFrom` with tokio
  types and `from_dyn` constructors stay. (D2)
- Removed: `Client::*_0rtt`, the `rpc` stub module, `oneshot::DynSender`,
  `read_request_raw`. (D3)
- `WithChannels` and `spans`: `#[non_exhaustive]` on the struct, or
  `tracing` always on. *open (D4)*
- `opentelemetry` moves to an in-repo `irpc-opentelemetry` 0.x crate
  behind a `Propagator` trait. *planned (D5.1)*
- Bounds review: drop `Unpin` and `Sync` where nothing needs them. (D7)
- Sealed: `IncomingRemoteConnection`, `RemoteConnection`, the varint
  traits.
- `#![deny(missing_docs, unreachable_pub)]`. (D6)

## 1.8 Renames

One pull request, last. *open per row, from E of the irpc plan*

| Today | 1.0 |
|---|---|
| `rpc_requests` | `rpc`, `rpc_service`, or unchanged (open) |
| `WithChannels { inner, tx, rx }` | `Call { request, tx, rx }` |
| `Request<L, R>` | `RequestSender<L, R>` |
| `Client::boxed` | `Client::remote` |
| `irpc_iroh::client(..)` | `Client::iroh(..)` (branch) |
| `IrohLazyRemoteConnection` | `LazyConnection` |
| `irpc::Sender`, `irpc::Receiver` | `TxChannel`, `RxChannel` |
| `is_rpc()` | `is_remote()` |
| `with_remote_channels(self, rx, tx)` | `(self, tx, rx)` |
| `RequestError::Connection` | `RequestError::OpenStream` |
| `Service`, `RemoteService` | `Protocol`, `RemoteProtocol` (undecided) |
| `RpcMessage` | `Payload` (undecided) |

`rpc_service` would not clash with the module `irpc::rpc`. A plain
`rpc` would. The variant attribute stays `#[rpc(tx = .., rx = ..)]`.

## 1.9 After 1.0

Additive, not blockers: `Handler::per_connection`,
`Client::with_timeout`, `Client::server_streaming_stream` (a `Stream`
of items), `Handler::from_client`, a per-protocol `MAX_MESSAGE_SIZE`
constant on `Service`, `LazyConnection::from_fn`, call-kind traits
for generic helpers (`RpcRequest<S>`), a generated client, mounts by
index, capability exchange, Unix sockets, rate limits.

# Part 2: irpc-schema

## 2.1 Schemas

```rust
use irpc_schema::HasSchema;

pub const SCHEMA_PREFIX: &str = "iroh-blobs";

#[derive(Debug, Serialize, Deserialize, HasSchema)]
#[schema(prefix = SCHEMA_PREFIX)]             // "iroh-blobs::Get"
pub struct Get {
    pub hash: Hash,
}

#[derive(Debug, Serialize, Deserialize, HasSchema)]
#[schema(name = "iroh-blobs::GetRequest")]    // renamed type, old hash kept
pub struct GetV2 {
    pub hash: Hash,
}

#[derive(Debug, Serialize, Deserialize, HasSchema)]
#[schema(structural)]                         // same schema as (u64, u64)
pub struct Range {
    pub start: u64,
    pub end: u64,
}

#[derive(Debug, Serialize, Deserialize, HasSchema)]
pub struct Authorized<T> {                    // generic: T: HasSchema
    pub cap: CapId,
    pub request: T,
}

#[derive(Debug, Serialize, Deserialize, HasSchema)]
pub struct Entry {
    #[schema(atom = "iroh::EndpointId")]      // foreign type without an impl
    pub peer: EndpointId,
    pub added: Duration,
}
```

| Item | Status |
|---|---|
| `#[derive(HasSchema)]`, nominal by type name | branch (`Frando/irpc-dev`) |
| `#[schema(..)]` helper on the derive: `nominal` (default), `structural`, `atom` | planned |
| `name = <expr>`, `prefix = <expr>`: any `&'static str` expression | planned |
| Generic types, with `T: HasSchema` bounds | planned |
| Field override `#[schema(atom = "..")]` or `#[schema(with = path)]` | planned |
| Impls for arrays, `Duration`, `SystemTime`, `PathBuf`, `SocketAddr`, larger tuples | planned |
| Impls for iroh types (`EndpointId`, `Hash`) behind an `iroh` feature | planned |
| The attribute macro `#[schema(Nominal)]` | kept, deprecated once the derive covers it |

Naming rules: the default name is the type name. A prefix belongs to
the type, in the crate that defines it, so a type shared by several protocols
has one hash. `prefix = module_path!()` changes the hash when the type
moves.

## 2.2 Schema-tagged services

Add `serialize_service` below the irpc macro:

```rust
#[irpc::rpc_requests(message = StoreMessage)]
#[irpc_schema::serialize_service]
#[derive(Debug)]
pub enum StoreProtocol {
    #[rpc(tx = oneshot::Sender<()>)]
    Set(Set),                                 // Set: HasSchema
    #[rpc(tx = oneshot::Sender<Option<String>>)]
    #[wrap(Get, derive(HasSchema))]
    Get { key: String },
}
```

- irpc writes every request as `(hash, payload)`. The hash covers the
  request type and the types of its `tx` and `rx` channels. Order,
  inserts, removals, and `#[cfg]` variants are wire-safe.
- Responses and stream items carry no hash.
- `#[serialize_service(channels = MyService)]` names another type with
  the `Channels` impls. The default is the enum.
- *branch (`Frando/irpc-dev`)*

## 2.3 Merged services

```rust
#[irpc_schema::merge_services(message = AppMessage)]
#[derive(Debug)]
pub enum AppProtocol {
    Store(StoreProtocol),
    Ping(PingProtocol),
}

// server: generated bundle, one handler per service
let handler = AppHandler {
    store: store.local_sender().into(),
    ping: ping_handler(),
};
router.accept(APP_ALPN, IrohProtocol::new(handler));

// client: plain clients of each service, one connection
let conn = IrohLazyRemoteConnection::new(endpoint, addr, APP_ALPN.to_vec());
let store = Client::<StoreProtocol>::remote(conn.clone());
let ping = Client::<PingProtocol>::remote(conn);
```

| Item | Status |
|---|---|
| `merge_services`: serde, `StableEnum`, `AppMessage`, `Service`, `RemoteService` | branch |
| Generated `AppHandler` with `From<AppHandler> for Handler<AppProtocol>` | planned |
| Shared hash: the first variant wins | branch |
| Shared hash from different Rust types: error | planned |
| `AppProtocol::validate() -> Result<(), Collision>`, called by `From<AppHandler>` and the hash lock | planned |

Build the bundle inside `ProtocolHandler::accept` to share
per-connection state across the services.

## 2.4 Versioning

Released protocols never change. Lock their hashes in a test:

```rust
#[test]
fn v1_is_frozen() {
    irpc_schema::assert_schema_hashes::<v1::StoreProtocol>(&[
        ("Set", "<hex of the released hash>"),
        ("Get", "<hex of the released hash>"),
    ]);
}
```

*planned.* A snapshot file variant prints the schemas too.

Serve old and new clients on one ALPN. List the newest first. Forward
old requests to the new actor:

```rust
#[irpc_schema::merge_services(message = StoreServerMessage)]
enum StoreServer {
    V2(v2::StoreProtocol),
    V1(v1::StoreProtocol),
}

async fn forward_v1(store: &LocalSender<v2::StoreProtocol>, msg: v1::StoreMessage) -> irpc::Result<()> {
    match msg {
        v1::StoreMessage::Get(WithChannels { inner, tx, .. }) => {
            let tx = tx.with_map(|value: Option<Bytes>| value.and_then(utf8));
            store.send((v2::Get { key: inner.key.into() }, tx)).await?;
        }
        ..
    }
    Ok(())
}
```

Or keep one ALPN per version (`store/1`, `store/2`), and let the
client offer both: `ConnectOptions::with_additional_alpns`, then
`conn.alpn()`. No library support needed.

Rules:

- A field change is a new type. A new request is a new variant.
- A response or error enum is part of the hash. Freeze it per version,
  or mark it `#[schema(atom, name = "..")]` to allow appended variants
  without a hash change. *open: maybe a dedicated `open` kind.*
- A new client against an old server gets `is_unsupported()` (needs
  C1), and can fall back to the old request.

See `examples/upgrade.rs` and `examples/versioning/`.

## 2.5 Hash contract

`hash = blake3(postcard(Schema))`, 32 bytes. Frozen once a crate ships
hashes:

- the order of the `Schema` variants (new ones go last),
- the atom names,
- the nominal names.

A test pins the hashes of reference types. *planned*

Decide before the first release: *open*

- One atom per wire encoding: `Vec<u8>`, `&[u8]`, and `Bytes` become
  `bytes`, and `String` and `&str` become `str`. Changes every hash that contains them.
- Hash length: 32, 16, or 8 bytes.

## 2.6 Later

- A generated client with one method per request
  (`client = StoreClient`), first in irpc-schema, then
  in irpc. *1.x*
- A meta protocol that lists the hashes a server knows. Needs C1.
  *1.x, optional*

# Part 3: Plan

## irpc-schema, now (on the irpc branch `Frando/feat-handler`)

- [x] Stable enum refactor, macro hygiene, `StableEnum`, `#[flatten]`
- [x] First variant wins for shared hashes
- [x] `#[derive(HasSchema)]`, `serialize_service(channels = ..)`, `merge_services`
- [x] Examples `compose` and `upgrade`
- [ ] Crate docs and README for stable enums, services, and merging
- [ ] Pin the hashes of the examples and of reference types
- [ ] `assert_schema_hashes`
- [ ] Derive helper attribute, `name`, `prefix`
- [ ] Generics in the derive
- [ ] Foreign types: built-in impls, field override, iroh feature
- [ ] `#[cfg]` on variants in the stable enum macros
- [ ] Same-type rule and `validate()` for shared hashes
- [ ] Generated `AppHandler` for merged services
- [ ] Example in the common shape: hand-written structs with the derive,
      an actor, auth first

## irpc 1.0

One pull request each, in this order:

- [x] P1, P2, B2.1
- [ ] A1 to A3, A4.1, E14 (`Frando/refactor-merge-iroh`)
- [ ] C2, C5, C6 (`Frando/feat-handler`)
- [ ] F1 to F7 (bug fixes), B2.2 (`crate = ..`), B3 (macro bugs)
- [ ] `#[cfg]` on variants, `From` for tuple wraps, `name()`
- [ ] D1 `#[non_exhaustive]`
- [ ] C1 per-stream rejection, `ReadRequestError`, `is_unsupported`
- [ ] Error classification, timeout case, stream abort case, reset on
      panic, `into_stream` stops after an error
- [ ] Seal `IncomingRemoteConnection` and `RemoteConnection`, typed
      remote id
- [ ] `Client::on_connect`, `IrohLazyRemoteConnection::connect`
- [ ] C3 tracing fixes
- [ ] A4.2 to A4.5, A5, A1.5 (irpc-iroh shim)
- [ ] D2 to D8
- [ ] Renames (1.8)
- [ ] G1 to G5 docs, then `1.0.0-rc.0`, port iroh-blobs, iroh-gossip,
      iroh-docs, iroh-services, then 1.0

## irpc-schema on irpc 1.0

- [ ] Port to irpc 1.0
- [ ] Split `irpc-schema-core`. A `ChannelSchema` trait in irpc-schema
      replaces the `HasSchema` impls on irpc channel types. Pinned
      hashes must not change.
- [ ] Decide atom normalization and hash length (2.5)
- [ ] Generated client, meta protocol (2.6)

# Open questions

1. Name of the schema core crate.
2. Drop `IrohProtocol::sequential` and `IrohProtocol::concurrent` from
   1.0?
3. Seal `RemoteConnection`, and offer `LazyConnection::from_fn` for
   pools?
4. `WithChannels` and `spans` (D4).
5. Each row of the renames table.
6. The derive helper attribute name: `#[schema(..)]` clashes with the
   attribute macro `schema` when a module imports both. Proposed: keep
   `#[schema(..)]` and deprecate the attribute macro.
7. Response enums that grow: `atom` with a pinned name, or an `open`
   kind.
8. Atom normalization and hash length.
9. irpc-iroh 0.18 shim, or stop at 0.17.
