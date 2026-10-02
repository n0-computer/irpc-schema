# RFD: irpc 1.0, the iroh transport, and irpc-schema

Status: draft for discussion. Date: 2026-09-29.

## TODO

- [x] write RFD and plan
- [ ] agree on the crate layout (section 1) and the schema core crate name
- [ ] agree on the irpc change list (section 2)
- [x] consumer survey (`plans/survey.md` in the main worktree)
- [ ] phase 1: irpc-schema 0.2 (in progress on `Frando/irpc-dev`)
- [ ] phase 2: split the schema core crate out of irpc-schema
- [ ] phase 3: irpc 1.0 changes, one pull request per item
- [ ] phase 4: irpc-schema on irpc 1.0, then the optional extras
- [ ] final review

## Summary

We want irpc 1.0 to be small and stable, and we want schema hashes,
protocol composition, and versioning to be standard features, not
things every user builds by hand. These two goals fit together because
the schema support works at the serde level: a protocol enum whose
serde impl writes a schema hash in front of each request needs nothing
from irpc. So we propose:

- irpc 1.0 changes little. It absorbs irpc-iroh behind a default `iroh`
  feature, renames `rpc_requests` to `rpc_service`, fixes macro
  hygiene, stops closing connections on unknown requests, and replaces
  the `Arc<dyn Fn>` handler with a `Handler` type that covers the
  standard cases.
- A new schema core crate (working name `irpc-schema-core`) holds
  `Schema`, `HasSchema`, the stable enum machinery, and the frozen hash
  contract. It does not depend on irpc.
- irpc-schema stays 0.x and becomes the integration layer: one
  attribute for schema-tagged protocols, one attribute to combine
  protocols, generated handler and client bundles, and versioning
  helpers. It can change faster than irpc because irpc does not depend
  on it.

## Background

### What exists

- irpc 0.17: `Service`, `Channels`, `WithChannels`, `Client`, the
  channel types, the noq transport in `irpc::rpc`, and the
  `rpc_requests` macro from irpc-derive.
- irpc-iroh 0.17: the iroh transport. It duplicates the server loop of
  `irpc::rpc` (`read_request_inner`, `read_request`, `read_request_raw`,
  `handle_connection`, `listen`) with a different connection type.
- irpc-schema 0.1: `Schema`, `HasSchema`, the `schema` attribute, and
  `serialize_stable` / `serialize_service`, which write each request as
  `(blake3(schema), payload)`. The irpc support is behind a feature.

### What this session showed

The details are in `plans/plan-api.md`. In short:

- Two protocols combine on one ALPN with no irpc change, if both use
  `serialize_service`: the schema hash identifies the request, so the
  server can find the right sub-protocol, and plain sub-protocol
  clients work unchanged (`Frando/feat-flatten`, the `combined`
  example).
- Versioning is the same mechanism: flatten a frozen v1 protocol and
  the current v2 protocol into one server protocol, and serve v1 with
  the public v2 API (`Frando/feat-versioning`, the `versioning`
  example).
- The gaps are small and concrete: macro hygiene (F1, F5), `wrap` types
  without `HasSchema` (F2), `schemas()` not reachable from generic code
  (F4), hand-written `Handler` closures for every combination, and an
  unknown request closes the whole connection with an error the client
  cannot recognize (F9).

### How consumers use irpc today

A survey of the iroh ecosystem crates on 0.17:

| Pattern | Where | Notes |
|---|---|---|
| `remote_handler(local)` + `listen` or `IrohProtocol::with_sender` | iroh-blobs, iroh-gossip, blobtorrent, valisk | The standard path. Works well. |
| Hand-written `Handler` closure | iroh-docs `src/api.rs:83` | 30 arms of `local.send((msg, tx))`. Only boilerplate. |
| Manual loop with `read_request` | iroh-services `client_host.rs`, svc `backend/src/rpc.rs:256`, svc `backhaul.rs` | First request must be `Auth`, checked against `connection.remote_id()`. Then a loop with per-connection state (capability, rate limiter, metrics decoder). Violations close the connection with a code. |
| Versioning | svc `svc-backhaul/src/relay_v1.rs` | A frozen v1 enum on ALPN `.../relay/1` "with the original postcard variant order", next to v2 on `.../relay/2`. "It shares only `Register` and `GetConfig` ... which are byte-identical there." |

Two conclusions follow. The `Arc<dyn Fn>` handler is only used where
the standard path does not reach. And `read_request` is the tool for
connections that need a handshake or per-connection state, so it must
stay.

### Related drafts in the irpc repo

`irpc/plans/irpc-next.md` (July 2026) proposes composition by index
prefix (A), 0-RTT capability negotiation (B), optional schema
discriminators (C), and a versioning recipe (D).
`irpc/plans/rename-call.md` and `irpc/plans/review-naming.md` propose
renames (`WithChannels` to `Call`, `Service` to `Protocol`, and more).
The section "Relation to irpc-next.md" compares this RFD with A to D.
The renames are out of scope here, but they should land in the same
breaking release as the changes below.

## Goals

- irpc 1.0 is small, stable, and changes as little as possible.
- iroh is the default transport, in the irpc crate itself.
- Schema-tagged protocols, combined protocols, and versioned protocols
  are standard: the common case needs no `Arc<dyn Fn>`, no hand-written
  read loop, and no restated channel types.
- The schema support can change after irpc 1.0 without an irpc major
  release.
- The schema hash of a type is a wire contract. It stays the same
  across releases of the schema crates.

## Non-goals

- Cross-language interop.
- A self-describing wire format. postcard stays.
- Capability negotiation inside irpc 1.0. It can come later as an
  addition (see section 6 and "Relation to irpc-next.md").
- The naming changes from `review-naming.md`. They are compatible with
  this RFD but decided elsewhere.

## Proposal

### 1. Crate layout

| Crate | Depends on | Stability | Contents |
|---|---|---|---|
| `irpc` | `irpc-derive`, noq, iroh (feature) | 1.0 | Core types, channels, noq transport (`irpc::rpc`), iroh transport (`irpc::iroh`), `#[irpc::rpc_service]` |
| `irpc-derive` | | 1.0 | The `rpc_service` macro |
| `irpc-schema-core` (working name) | serde, postcard, blake3 | 0.x API, frozen hash contract | `Schema`, `HasSchema`, `#[derive(HasSchema)]`, `StableEnum`, `#[stable]` (today `serialize_stable`) with `#[flatten]`, hash lock helpers |
| `irpc-schema` | `irpc` 1, `irpc-schema-core` | 0.x | `ChannelsSchema`, `#[irpc_schema::rpc_service]` (with `merge`), generated handler and client bundles, versioning helpers, optional meta protocol |
| `irpc-schema-derive` | | 0.x | Proc macros for both schema crates |

```text
irpc-schema ----> irpc 1.0 ----> irpc-derive
     |
     +----------> irpc-schema-core
```

irpc does not depend on either schema crate. This is what lets the
schema crates stay 0.x: an irpc 1.0 that exposed `HasSchema` in its
public API would need a major release for every breaking change in the
schema core.

One consequence: today irpc-schema implements `HasSchema` for the irpc
channel types (`oneshot::Sender<T>` and friends). Without an irpc
dependency, the core crate cannot do that, and irpc-schema cannot
either (both the trait and the types would be foreign). irpc-schema
gets its own trait for channel schemas instead:

```rust
/// Returns the schema of an irpc channel type, for `ChannelsSchema`.
pub trait ChannelSchema {
    fn schema() -> Schema;
}

impl<T: HasSchema> ChannelSchema for irpc::channel::oneshot::Sender<T> {
    fn schema() -> Schema {
        // Must stay equal to the 0.1 output, or every service hash changes.
        Schema::named("irpc::channel::oneshot::Sender", T::schema())
    }
}
```

`ChannelsSchema<S>` then builds the same `Schema` value as today, so
the hashes do not change. A test pins the current hashes before the
split (see the plan).

### 2. Changes to irpc

Each item is small and can be one pull request. Items I1 to I6 are the
proposal. I7 to I9 are clean-ups that are cheap now and expensive after
1.0.

#### I1: move irpc-iroh into `irpc::iroh`, behind a default `iroh` feature

iroh is at 1.0, and iroh re-exports the noq stream types
(`iroh::endpoint::{SendStream, RecvStream}` are `noq::SendStream` and
`noq::RecvStream`), so the handler and channel code already works for
both transports. Only the connection types differ.

- `irpc::iroh::{IrohProtocol, Iroh0RttProtocol,
  IrohLazyRemoteConnection, read_request, ...}` keep their names. A
  later rename can drop the `Iroh` prefix. `Frando/feat-handler` drops
  the `client` function: clients use
  `Client::boxed(IrohLazyRemoteConnection::new(..))`. A short
  constructor such as `Client::iroh(endpoint, addr, alpn)` would help
  the common case.
- One generic server loop instead of two. Extend the existing
  `IncomingRemoteConnection` trait to noq connections. The only
  iroh-specific part, `remote_id()`, becomes a peer label for tracing.
- `impl RemoteConnection for iroh::endpoint::Connection` replaces the
  `IrohRemoteConnection` wrapper, as the TODO in irpc-iroh suggests.
  Keep the wrapper as a deprecated alias for one release.
- Features: `iroh = ["rpc", "dep:iroh", "dep:portable-atomic"]`, in
  `default`. Remove `noq_endpoint_setup` from `default`: it pulls in
  rustls with ring and is a test helper.
- irpc-iroh gets a last release that re-exports `irpc::iroh` with a
  deprecation note.

Usage after the change:

```rust
use irpc::iroh::{IrohLazyRemoteConnection, IrohProtocol};

let api = StoreApi::spawn();
let router = Router::builder(endpoint.clone())
    .accept(StoreApi::ALPN, IrohProtocol::new(api.local_sender()))
    .spawn();
let conn = IrohLazyRemoteConnection::new(endpoint, addr, StoreApi::ALPN.to_vec());
let client = irpc::Client::<StoreProtocol>::boxed(conn);
```

Cost: an iroh 2.0 forces an irpc 2.0. The noq types in the public API
already tie irpc to noq majors, so this adds little.

#### I2: rename `rpc_requests` to `rpc_service`

```rust
#[irpc::rpc_service(message = StoreMessage)]
#[derive(Debug, Serialize, Deserialize)]
enum StoreProtocol {
    #[rpc(tx = oneshot::Sender<Option<String>>)]
    Get(Get),
}
```

The macro defines a service, so the name says so. A plain `rpc` would
clash twice: with the module `irpc::rpc` (Rust allows a macro and a
module with the same name, but `use irpc::rpc;` then imports both), and
with the variant attribute `#[rpc(tx = ..)]`. `rpc_service` avoids both.
irpc-schema uses the same name for its schema-tagged variant
(section 4), so a protocol switches between the two by its attribute
path only. Keep `rpc_requests` as a deprecated re-export for one
release.

#### I3: macro hygiene

The macro output uses `::tracing` and, with `wrap`, `::serde`, so every
user crate must depend on both. Re-export them from a hidden module and
use those paths:

```rust
#[doc(hidden)]
pub mod __private {
    pub use serde;
    #[cfg(feature = "spans")]
    pub use tracing;
}
```

irpc-schema did the same on `Frando/feat-flatten`.

#### I4: reject unknown requests per stream, not per connection

Today `read_request_inner` fails when postcard cannot decode the
request frame, and `handle_connection` then ends the connection. Every
other request on that connection fails with it, and the client of the
bad request sees `Io error: failed to read size`. We reproduced this
with a v1 client against a v2-only server (F9 in `plan-api.md`).

Change: when the frame does not decode, reset the send stream and stop
the receive stream with a new code, log a warning, and accept the next
stream.

```rust
pub const ERROR_CODE_INVALID_REQUEST: u32 = 3;

// in the one generic server loop
match postcard::from_bytes::<S>(&buf) {
    Ok(msg) => return Ok(Some((msg, recv, send))),
    Err(err) => {
        warn!("rejected request that does not decode: {err}");
        send.reset(ERROR_CODE_INVALID_REQUEST.into()).ok();
        recv.stop(ERROR_CODE_INVALID_REQUEST.into()).ok();
        continue;
    }
}
```

On the client, irpc maps a stream reset with this code to an
`io::Error` of kind `Unsupported` inside the existing `Io` variants. That is not a
breaking change. With I6, it can become a variant of its own.

This helps the manual loops in iroh-services and svc too: a newer
client that sends an unknown request no longer ends the session.

#### I5: standard handlers without `Arc<dyn Fn>`

Status: implemented on the irpc branch `Frando/feat-handler`. irpc-schema
builds against it on `Frando/irpc-dev`.

`Handler<S>` becomes a struct. The `Arc<dyn Fn>` is an internal detail.
The constructors cover the standard cases:

- `Handler::from_sender(local)`, and `From<LocalSender<S>>`: forward to
  an actor.
- `Handler::sequential(f)` and `Handler::concurrent(n, f)`: run `f` on
  the message enum, one request at a time or up to `n` per connection.
- `Handler::raw(f)`: run `f` on the protocol enum and the streams.
  `Handler::call` passes a request on to another handler, which is what
  combined protocols need.

A handler returns `Err(CloseConnection::new(code, reason))` to close the
connection. Every function that takes a handler accepts
`impl Into<Handler<S>>`:

```rust
router.accept(ALPN, IrohProtocol::new(api.local_sender()));
router.accept(ALPN, IrohProtocol::sequential(|msg| async move { .. }));
```

The iroh-docs closure becomes `IrohProtocol::new(local)`. For combined
protocols, the dispatch is a `Handler::raw` with one `call` per
sub-protocol (see the `compose` example), and irpc-schema can generate
it (section 5).

Why not a handler trait with a generic `IrohProtocol<S, H>`: it adds a
type parameter to every server type, and the dynamic call costs one
virtual call per request, which is negligible next to a QUIC stream.

#### I6: `#[non_exhaustive]` on public error enums and opaque channel enums

`RequestError`, `Error`, `WriteError`, `SendError`, and the channel
`RecvError` types are not `#[non_exhaustive]` in 0.17. Adding a variant
after 1.0 would be a breaking change. Mark them now (if `stack_error`
allows it, otherwise wrap). The same applies to the public variants of
`mpsc::Sender`, `mpsc::Receiver`, `oneshot::Sender`, and
`oneshot::Receiver` (`Tokio`, `Boxed`): make these types opaque structs,
because `oneshot::Receiver::Tokio` already exposes a crate-private type.

#### I7: keep `read_request`, in one place

The survey shows that `read_request` is the right tool for connections
with a handshake or per-connection state:

```rust
let Some(first) = irpc::iroh::read_request::<ServicesProtocol>(&conn).await? else {
    return Ok(());
};
let ServicesMessage::Auth(WithChannels { inner, tx, .. }) = first else {
    conn.close(400u32.into(), b"expected auth");
    return Ok(());
};
// check inner against conn.remote_id(), then serve the session:
while let Some(msg) = irpc::iroh::read_request::<ServicesProtocol>(&conn).await? {
    session.handle(msg).await?;
}
```

A free function fits this loop. A `Requests<S>` stream type or a
`ConnectionExt::read_request` method would read about the same and add
API. So: keep the functions, move them into `irpc::iroh` and
`irpc::rpc` on top of the one generic implementation from I1, and
remove the duplicates. `read_request_raw` stays for loops that need the
protocol value before irpc attaches the channels.

#### I8: small fixes to take along

- `IrohProtocol::accept` creates `trace_span!("rpc", id)` without a
  `remote` field, so `handle_connection` cannot record the peer. Add
  the field.
- The `Display` output of `Error::Write` is "Recv error" (`src/lib.rs:943`).
- irpc-iroh depends on `iroh-base` without using it.
- Both library crates declare `crate-type = ["cdylib", "rlib"]`. Check
  whether the `cdylib` is still needed.

#### I10: connection context for handlers (from the survey)

The most common reason for a manual `read_request` loop is that a
handler cannot see its connection. Eight surveyed projects need at
least one of these: the remote id, an auth request that must come
first, per-connection state (capabilities, rate limiters, decoders),
and a hook when the connection ends. `Handler` on `Frando/feat-handler`
does not cover this yet.

A minimal shape: a handler that runs a setup step once per connection
and returns the per-request function.

```rust
let handler = Handler::<ServicesProtocol>::per_connection(|conn| async move {
    // Runs once per connection, before the first request.
    let remote = conn.remote_id();
    let mut session = Session::new(remote);
    Ok(move |msg| session.handle(msg)) // sequential, with &mut state
});
```

An auth-first variant reads the first request in the setup step:

```rust
let handler = Handler::<ServicesProtocol>::per_connection(|conn| async move {
    let ServicesMessage::Auth(auth) = conn.first_request().await? else {
        return Err(CloseConnection::new(400, "expected auth"));
    };
    let caps = verify(conn.remote_id(), &auth.inner)?;
    auth.tx.send(()).await.ok();
    Ok(move |msg| serve(&caps, msg))
});
```

This is additive, so it can also come in 1.x. The manual loops keep
working with `read_request` (I7) until then.

#### I11: re-run auth after a lazy reconnect (from the survey)

`IrohLazyRemoteConnection` reconnects silently. A protocol that
authenticates the connection with a first request then sends its next
request on a connection without auth. Five surveyed projects hit this:
one has a live bug, others stopped using the lazy connection. A hook
that runs on every new connection fixes it:

```rust
let conn = IrohLazyRemoteConnection::new(endpoint, addr, ALPN.to_vec())
    .on_connect(|client: Client<ServicesProtocol>| async move {
        client.rpc(Auth { caps }).await
    });
```

#### I9: what irpc does not get

- No dependency on the schema crates and no `schema` feature.
- No `Mount` trait, no request framing prefix, and no capability
  exchange for 1.0. All three are additive and can come in a 1.x
  release if needed (see "Relation to irpc-next.md").

### 3. The schema core crate

Working name `irpc-schema-core`. Contents:

- `Schema`, `Named`, `HasSchema`, `SchemaAndHash`, `stable_hash`.
- `#[derive(HasSchema)]` with an optional helper attribute, next to the
  current `schema` attribute macro (P2 in `plan-api.md`). The derive
  makes types from `wrap` work:

  ```rust
  #[derive(Debug, Serialize, Deserialize, HasSchema)]
  #[schema(nominal, name = "store::Put")]
  pub struct Put {
      pub key: String,
      pub value: String,
  }
  ```

- `StableEnum` and `#[stable]` (today `serialize_stable`) with
  `#[flatten]`, as on `Frando/feat-flatten`.
- Hash lock helpers for released protocols:

  ```rust
  #[test]
  fn v1_protocol_is_frozen() {
      irpc_schema_core::assert_schema_hashes::<v1::StoreProtocol>(&[
          ("Put", "69289a74fb5f4af5fefdc21301ff95948262d49677beb4e073e2747db651a783"),
          ("Get", "ed316039ad0159e4db98762b811d01c39f6de0300c28061c75111f5991a819a7"),
          ("List", "ad968aab216e7f87edd1c4039799df7c6b9c78a50e2da48f76e2d6e656fe8b78"),
      ]);
  }
  ```

  A snapshot file variant (pretty-printed schemas plus hashes, checked
  in, updated on request) also shows what changed.

#### The hash contract

The hash of a type is `blake3(postcard(Schema))`. That makes these
things part of the wire format:

- The order of the `Schema` enum variants. New variants go at the end.
- The atom names. Today they come from `stringify!` on the type, so
  `String`, `&str`, and `bytes::Bytes` are three different atoms.
- The encoding of nominal types: only the type name, not the module
  path. A rename changes the hash unless `name = ".."` pins it.
- The hash function and the hash length.

Before the hash contract is frozen, we should decide two things:

- Normalize atoms of types that are equal on the wire. In postcard,
  `Vec<u8>`, `&[u8]`, and `bytes::Bytes` all encode as a length and
  the bytes, and `String` and `&str` both encode as a length and UTF-8
  bytes. Today a switch from `Vec<u8>` to `Bytes` changes the hash
  although no byte on the wire changes. One atom per wire encoding
  (for example `bytes` and `str`) fixes that. This changes existing
  hashes once, so it must happen before users depend on them.
- The hash length. 32 bytes per request is 31 more than a postcard
  discriminant. postcard-rpc uses an 8-byte FNV-1a key of the path and
  the schema, with 1, 2, and 4-byte forms. A 16-byte truncation of
  blake3 halves the cost and still makes collisions between the types
  of one program negligible. Also see open questions.

The core crate documents the contract, and a test pins the hashes of a
set of reference types, so that an accidental change fails CI.

### 4. irpc-schema: one attribute per protocol

Today a schema-tagged protocol stacks two attributes, repeats the enum
name, and depends on the attribute order:

```rust
#[rpc_requests(message = StoreMessage)]
#[serialize_service(StoreProtocol)]
#[derive(Debug)]
pub enum StoreProtocol { .. }
```

Proposal: irpc-schema provides one attribute that expands to both, in
the right order, and adds the `HasSchema` derive to `wrap` types:

```rust
#[irpc_schema::rpc_service(message = StoreMessage)]
#[derive(Debug)]
pub enum StoreProtocol {
    #[rpc(tx = oneshot::Sender<()>)]
    #[wrap(Put)]
    Put { key: String, value: String },
    #[rpc(tx = oneshot::Sender<Option<String>>)]
    #[wrap(Get)]
    Get { key: String },
    #[rpc(tx = mpsc::Sender<(String, String)>)]
    #[wrap(List)]
    List,
}
```

It expands to:

```rust
#[::irpc::rpc_requests(message = StoreMessage)]
#[::irpc_schema::serialize_service(StoreProtocol)]
#[derive(Debug)]
pub enum StoreProtocol {
    #[rpc(tx = oneshot::Sender<()>)]
    #[wrap(Put, derive(::irpc_schema::HasSchema))]
    Put { key: String, value: String },
    ..
}
```

irpc needs nothing for this. The user writes the same protocol as with
plain irpc and gets schema hashes by changing the attribute path.

Status: implemented on `Frando/irpc-dev` as `#[irpc_schema::rpc_service]`,
together with `#[derive(HasSchema)]`. See the `compose` and `upgrade`
examples.

### 5. irpc-schema: merged protocols

Status: `Frando/irpc-dev` implements `#[rpc_service(merge, message = ..)]`
with the message enum, `Service`, and `RemoteService`. The server still
writes the `Handler::raw` dispatch by hand (see the `compose` example).
The handler and client bundles below are proposals.

```rust
#[irpc_schema::rpc_service(merge, message = AppMessage)]
#[derive(Debug)]
pub enum AppProtocol {
    Ping(PingProtocol),
    Store(StoreProtocol),
}
```

Every variant holds a protocol, so the macro treats each variant as
`#[flatten]`. A dedicated attribute name (`combine`, `rpc_router`) was
considered: `merge` keeps one attribute for every service definition,
and `rpc_router` would read like iroh's `Router` in the same file. The
macro generates:

- the serde impls and `StableEnum`, as for flattened stable enums,
- `enum AppMessage { Ping(PingMessage), Store(StoreMessage) }`,
  `Service`, and `RemoteService`,
- a handler bundle that converts into `Handler<AppProtocol>`. Its
  `From` impl is the `Handler::raw` dispatch from the `compose` example:

  ```rust
  pub struct AppHandler {
      pub ping: irpc::rpc::Handler<PingProtocol>,
      pub store: irpc::rpc::Handler<StoreProtocol>,
  }
  impl From<AppHandler> for irpc::rpc::Handler<AppProtocol> { .. }
  ```

- a client bundle that shares one connection:

  ```rust
  #[derive(Debug, Clone)]
  pub struct AppClient {
      pub ping: irpc::Client<PingProtocol>,
      pub store: irpc::Client<StoreProtocol>,
  }
  impl AppClient {
      pub fn remote(conn: impl irpc::rpc::RemoteConnection + Clone) -> Self { .. }
  }
  ```

The names are `{Enum without "Protocol"}Message`, `..Handler`, and
`..Client` by default, and can be set with arguments
(`message = .., handler = .., client = ..`).

Usage:

```rust
// Server: each sub-protocol keeps its own actor.
let handler = AppHandler {
    ping: ping.local_sender().into(),
    store: store.local_sender().into(),
};
let router = Router::builder(endpoint.clone())
    .accept(PingApi::ALPN, IrohProtocol::new(ping.local_sender()))
    .accept(APP_ALPN, IrohProtocol::new(handler))
    .spawn();

// Client: both sub-clients on one connection.
let conn = IrohLazyRemoteConnection::new(endpoint, addr, APP_ALPN.to_vec());
let app = AppClient::remote(conn);
app.ping.rpc(Ping { nonce: 1 }).await?;
app.store.rpc(Put { key, value }).await?;
```

A plain `Client<PingProtocol>` on `APP_ALPN` also works, because the
wire format of a combined protocol is the wire format of its parts.

#### Shared requests across sub-protocols

Status: `Frando/irpc-dev` implements "first wins".

The first implementation panicked when two flattened enums contained a
variant with the same hash. The svc code shows that this is normal for
versions: its v1 and v2 relay protocols share `Register` and
`GetConfig`, byte for byte. So now the first variant in declaration
order that knows a hash gets it, and `schemas()` lists each hash once.
Two payload variants of one enum with the same hash still panic.

The survey shows a gap in this rule. Unrelated protocols often contain
requests with the same name, no fields, and the same channels:
`Shutdown`, `List {}`, `WaitIdleRequest`, `GetConfig`. These hash the
same, so "first wins" would send one protocol's `Shutdown` to the other
protocol's handler, without an error. The shared requests between
versions are different: there both versions use the same Rust type.

Proposed rule: a shared hash is fine if all variants that know it hold
the same Rust type, and a panic otherwise. The stable enum macros know
the payload type of each variant, so `StableEnum` can report it:

```rust
pub trait StableEnum: Sized {
    fn schemas() -> impl Iterator<Item = (&'static str, &'static Schema, [u8; 32])>;
    /// Returns the schema hash and the type id of each payload type.
    fn payload_types() -> impl Iterator<Item = ([u8; 32], std::any::TypeId)>;
    fn deserialize_payload<'de, A: SeqAccess<'de>>(..) -> ..;
}
```

The other fix for accidental collisions is a name prefix per crate
(section 8). Both together give clear errors for accidents and keep
shared types working.

### 6. Versioning

The recipe combines three tools. Each works without the others.

1. Released protocols are frozen. A `v1` module keeps the exact types
   and channel declarations. A hash lock test (section 3) makes an
   accidental change fail CI. This matches svc's "this module never
   changes".
2. A server serves several versions on one ALPN with a merged
   protocol:

   ```rust
   #[irpc_schema::rpc_service(merge, message = StoreServerMessage)]
   pub enum StoreServer {
       V2(v2::StoreProtocol),
       V1(v1::StoreProtocol),
   }

   let v1_compat = v1_compat::spawn(store.clone()); // v1 requests on the v2 API
   let handler = StoreServerHandler {
       v2: store.local_sender().into(),
       v1: v1_compat.local_sender().into(),
   };
   router.accept(STORE_ALPN, IrohProtocol::new(handler));
   ```

   v2 has no v1 variants and restates no v1 channel types. The v1
   adapter uses only the public v2 API.
3. A client finds out what the server speaks. Three ways, cheapest
   first:
   - ALPN negotiation, no library change: the server accepts
     `store/2` and `store/1` with the same handler, and the client
     offers `store/2` first with `ConnectOptions::with_additional_alpns`
     and reads `Connection::alpn()`. This versions whole protocols and
     works against servers that exist today. irpc can add a helper
     later: `irpc::iroh::connect_negotiated(endpoint, addr, &[ALPN_2,
     ALPN_1]) -> (Vec<u8>, Connection)`.
   - Try and fall back, needs I4: send the v2 request, and on the
     "unsupported" error send the v1 request and remember the answer
     for the connection.
   - Ask the server, needs I4 to be safe against old servers: a small
     meta protocol in irpc-schema with one request that returns the
     hashes the server knows. A server adds it with one more variant
     in its merged protocol, and irpc-schema serves it for any
     `StableEnum`:

     ```rust
     #[irpc_schema::rpc_service(merge, message = StoreServerMessage)]
     pub enum StoreServer {
         Meta(irpc_schema::meta::MetaProtocol),
         V2(v2::StoreProtocol),
         V1(v1::StoreProtocol),
     }
     let meta = irpc_schema::meta::serve::<StoreServer>();
     ```

The schema hashes do not replace the ALPN. They make a mismatch a clean
error instead of a wrong decode, and they let one server serve several
versions of a protocol on one ALPN.

### 7. Other proposals

- Generated `schemas()` names for flattened variants: today a combined
  enum lists inner names only (`Ping`, `Put`). Add the path
  (`Ping/Ping`, `Store/Put`) in a separate method, to keep the current
  output stable.
- Generics: `#[derive(HasSchema)]` should support generic types, with a
  `T: HasSchema` bound per parameter.
- `required-features` for the examples, so that
  `cargo build --no-default-features --examples` works.
- Error codes: irpc defines 1 to 3. Reserve a range for irpc and
  document that application close codes (svc uses 400, 401) should stay
  outside it.

### 8. Nominal names

Today a nominal schema uses the type name. `#[schema(Nominal(name =
"v1::Get"))]` replaces it with a string literal. The derive and the
types from `rpc_service` always use the type name. That causes three
problems:

- Accidental collisions: unrelated protocols in one combined server can
  contain types with the same name and the same fields (section 5).
- Renames change the hash. A crate that renames `GetRequest` to `Get`
  breaks old peers, though no byte on the wire changes.
- A crate cannot state its naming once. A prefix like `iroh-blobs` must
  be repeated as a literal on every type, and cannot come from a
  `const`.

#### Proposal

The naming arguments take an expression of type `&'static str`, not
only a literal. `schema()` runs at runtime, so a literal, a `const`,
`concat!(..)`, and `module_path!()` all work.

- `name = <expr>` sets the full name.
- `prefix = <expr>` sets the name to `{prefix}::{TypeName}`.
- `rpc_service(.., schema_prefix = <expr>)` sets the prefix for all
  types that `#[wrap]` generates in that service. A `#[schema(..)]` on a
  variant overrides it for that type. `rpc_service` removes that
  attribute before `rpc_requests` sees it, and emits the `HasSchema`
  impls of the `wrap` types itself instead of adding the derive. It
  knows the struct name and the fields from the variant.

```rust
pub const SCHEMA_PREFIX: &str = "iroh-blobs";

#[derive(Debug, Serialize, Deserialize, HasSchema)]
#[schema(prefix = SCHEMA_PREFIX)] // "iroh-blobs::Get"
pub struct Get {
    pub hash: Hash,
}

#[derive(Debug, Serialize, Deserialize, HasSchema)]
#[schema(name = "iroh-blobs::GetRequest")] // renamed type, old name pinned
pub struct GetRequestV2 {
    pub hash: Hash,
}

#[irpc_schema::rpc_service(message = StoreMessage, schema_prefix = SCHEMA_PREFIX)]
#[derive(Debug)]
pub enum StoreProtocol {
    #[rpc(tx = oneshot::Sender<()>)]
    #[wrap(Put)] // "iroh-blobs::Put"
    Put { key: String, value: String },
    #[rpc(tx = oneshot::Sender<()>)]
    #[wrap(Shutdown)]
    #[schema(name = "iroh-blobs::store::Shutdown")]
    Shutdown,
}
```

Rules:

- The prefix belongs to the type, not to the protocol. A type that
  several protocols share (svc `Register`) gets its prefix where it is
  defined, so it keeps one hash in every protocol. A protocol-level
  prefix that renamed imported types would break that. `schema_prefix`
  therefore applies only to the `wrap` types of the service itself.
- Without `name` or `prefix`, the name stays the type name, so existing
  hashes do not change. Adding a prefix to a released type changes its
  hash. Do it only for new types, or for all types of a new protocol
  version.
- `prefix = module_path!()` is short, but moving a type to another
  module then changes its hash. The docs must say so.
- `Atom` takes the same `name` and `prefix` arguments.

#### The helper attribute name

`#[derive(HasSchema)]` with a helper attribute `#[schema(..)]` clashes
with the `schema` attribute macro: if a module imports both, Rust reports
the attribute as ambiguous. Two ways out:

- Name the helper `#[has_schema(..)]`. No clash, but two spellings.
- Make the derive the main API and deprecate the attribute macro. The
  helper then takes the kind as well: `#[schema(structural)]`,
  `#[schema(atom, name = "..")]`. Recommended, since the derive also
  works in `wrap`.

#### Steps

- [ ] Accept `name = <expr>` and `prefix = <expr>` in the `schema`
      attribute, keeping the `Nominal(name = "..")` form.
- [ ] Add the helper attribute to the derive, after the naming decision
      above.
- [ ] `rpc_service(schema_prefix = ..)` and per-variant `#[schema(..)]`,
      with generated impls for `wrap` types.
- [ ] Tests: a literal and an equal `const` give the same hash, `prefix`
      gives `{prefix}::{TypeName}`, and the pinned default hashes do not
      change.

## Relation to irpc-next.md

| irpc-next | This RFD | Change to irpc |
|---|---|---|
| A: mount sub-protocols by index prefix (`Frame` in the client, framed `prepare_write`, `Mount`, `#[rpc_router]`) | `#[irpc_schema::rpc_service(merge, ..)]`: the schema hash identifies the request, so sub-clients need no projection and the server needs no layout knowledge | None |
| B: capability advert on a control stream, cached per `EndpointId` | ALPN negotiation now, meta protocol later (section 6) | I4 only |
| C: `discriminator = schema` feature in irpc, negotiated compact table | `#[irpc_schema::rpc_service]` at the serde level, outside irpc | None |
| D: versioning recipe, "existing request structs and their `Channels` impls are immutable once shipped" | Same rule, enforced by hash lock tests (section 6) | None |

The tradeoffs:

- Size: index framing costs one byte per level. Schema framing costs
  32 bytes (or 16 with a shorter hash) per request. The negotiated
  table in C.2 would remove that, but it needs connection state during
  serialization, which is an irpc change. For most protocols the cost
  is small next to the QUIC stream overhead. For protocols where it
  matters, index composition (A) remains possible as an addition in
  1.x.
- Layout: index framing couples the client to the server's root enum
  order, which is why irpc-next needs `mount_index` in the advert.
  Schema framing has no such coupling.
- Robustness: an unknown hash is a clean rejection. An unknown index in
  a positional enum can decode as the wrong variant when two versions
  of an enum have the same shape.
- Open question 6 of irpc-next (the order of the schema hash and the
  span propagation tuple) has a clear answer in this design: the span wrapper
  is `(carrier, S)`, and `S` writes its own hash, so the wire is
  `(carrier, (hash, payload))`.

## Alternatives considered

- A `schema` feature in irpc. It couples irpc 1.0 to a 0.x crate, or
  forces the schema crates to 1.0 early.
- One flat enum with all requests of all sub-protocols. It compiles
  today, but it restates every `#[rpc(..)]` and loses the sub-clients
  (`plan-api.md`, "Rejected alternative").
- A byte-level mux in front of irpc (the first `combined` example on
  `Frando/api`). It copies the irpc read loop and loses span
  propagation and 0-RTT (F7).
- A handler trait with `IrohProtocol<S, H>` instead of I5 (see I5).

## Open questions

1. Name of the schema core crate: `irpc-schema-core`, `stable-schema`,
   or `schema-hash`. The first keeps the family name, the others make
   clear that it does not need irpc.
2. The variant attribute `#[rpc(tx = ..)]`: keep it, or rename it to
   `#[call(tx = ..)]` to fit the `Call` rename? The `rpc_service` name
   (I2) already avoids the clash with the module `irpc::rpc`.
3. Hash length: keep 32 bytes, or truncate to 16 or 8 before freezing?
4. Atom normalization (section 3): do it, and if so, which atom names?
5. Shared requests in merged protocols: is the type id rule in section 5
   enough, or should sharing need an opt-in attribute as well?
8. The helper attribute name for `#[derive(HasSchema)]` (section 8).
9. Join `prefix` and the type name with `::`, or concatenate them as
   given?
6. Should `merge` also generate a local `AppClient::local(..)` from
   the sub-protocol local senders, for symmetry with `remote`?
7. Should irpc-iroh's last release be a re-export shim, or should the
   crate stop at 0.17?

## Plan

### Phase 1: irpc-schema 0.2

This phase needs no irpc changes. `Frando/irpc-dev` builds it
against the irpc branch `Frando/feat-handler`, to use the new `Handler`.
Each step is one commit with tests.

- [ ] Land `Frando/feat-flatten` (refactor, hygiene, `StableEnum`,
      `#[flatten]`) after review.
- [x] Change duplicate hashes in flattened enums from panic to "first
      wins" (section 5), with a test for shared requests.
- [ ] The type id rule for shared hashes (section 5).
- [ ] Pin the current hashes of the example protocols and of a set of
      reference types in a test. Every later step must keep them.
- [x] `#[derive(HasSchema)]`, next to the attribute macro. The helper
      attribute waits for the naming design (section 8).
- [x] `#[irpc_schema::rpc_service]`, which expands to `rpc_requests`
      and `serialize_service`, and adds the derive to `wrap` types.
- [x] Single-file examples `compose` and `upgrade`, and crate docs.
- [ ] Nominal names (section 8).
- [x] `#[irpc_schema::rpc_service(merge, message = ..)]` with message
      enum, `Service`, and `RemoteService`.
- [ ] Handler bundle and client bundle for merged protocols. With the
      `Handler` of `Frando/feat-handler`, the bundle's `From` impl is a
      `Handler::raw` with one `call` per sub-protocol, and users fill
      the bundle with `local_sender.into()`.
- [ ] `assert_schema_hashes` helper.
- [ ] Rewrite both examples on the new attributes. The `combined`
      example loses `app.rs`'s hand-written handler, the `versioning`
      example loses `server.rs`'s closure.

### Phase 2: split the schema core crate

- [ ] Create `irpc-schema-core` with `Schema`, `HasSchema`,
      `StableEnum`, the stable enum macro, and the hash lock helpers.
- [ ] Replace the channel `HasSchema` impls in irpc-schema with the
      `ChannelSchema` trait (section 1). The pinned hashes must not
      change.
- [ ] irpc-schema re-exports the core crate, so that user code does
      not change.
- [ ] Decide atom normalization and hash length (open questions 3 and
      4). If either changes the hashes, do it in this phase, update
      the pinned hashes in one commit, and write it in the changelog.

### Phase 3: irpc 1.0

One pull request per item, in this order, because later items build on
earlier ones:

- [x] I3 macro hygiene (irpc main, #121).
- [ ] I4 per-stream rejection of unknown requests, with a test that
      sends an unknown request and checks that the connection stays
      up.
- [ ] I1 merge irpc-iroh into `irpc::iroh`, one generic server loop,
      `iroh` in default features, `noq_endpoint_setup` out of them.
      (Done on `Frando/feat-handler`.)
- [ ] I7 remove the duplicate `read_request` family (done on
      `Frando/feat-handler`).
- [ ] I5 `Handler` struct with `From<LocalSender<S>>` (done on
      `Frando/feat-handler`).
- [ ] I2 rename `rpc_requests` to `rpc_service`, with a deprecated
      alias.
- [ ] I10 connection context for handlers, unless it moves to 1.x.
- [ ] I11 reconnect hook on `IrohLazyRemoteConnection`.
- [ ] I6 `#[non_exhaustive]` errors and opaque channel enums.
- [ ] I8 small fixes.
- [ ] The renames from `review-naming.md`, if accepted, in the same
      release.

### Phase 4: irpc-schema on irpc 1.0

- [ ] Port to irpc 1.0. The handler bundle now implements
      `From<..> for Handler<..>`.
- [ ] Meta protocol (section 6), if a user needs exact capability
      checks.
- [ ] ALPN negotiation helper in `irpc::iroh`, if more than one crate
      needs it. Until then, an example.

### Risks

- Hash changes by accident during the split or the macro rewrites. The
  pinned hashes from phase 1 guard this.
- The attribute order in `#[irpc_schema::rpc_service]` depends on how
  `rpc_requests` treats unknown attributes and `wrap`. A test per
  macro argument guards this.
- iroh 2.0 forces irpc 2.0 after I1. We accept this.
- The 32-byte cost per request can matter for chatty protocols. Open
  question 3 addresses it.

### Commit strategy

- irpc-schema: one commit per checklist item, refactors before
  features, tests in the same commit as the code.
- irpc: one pull request per item, so each can be reviewed and reverted
  on its own. I1 is the largest. It can land as two commits: first the
  generic server loop in irpc with noq only, then the iroh module.
