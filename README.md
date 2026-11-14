# N0-Schema

This is a simple schema language for rust types. The intent is not to be a fully comprehensive schema language that supports code generation etc, but just to provide the ability to evolve a rust RPC api without losing compatibility or having to do versioning for every single minor change. The approach is similar to what is done in [postcard-rpc], but while postcard-rpc is using u64 hash of endpoint name and schema, this crate is using 32 byte [BLAKE3] hashes for schema. With a BLAKE3 hash a collision is not just extremely improbable but basically impossible.

Schema hashes are computed by postcard-encoding the schema and then computing the BLAKE3 hash.

# History

This crate has been developed initially for the iroh rpc crate [irpc]. But we found it useful in a number of other contexts, such as the [n0pe] streaming database.

# Deriving schemas

There is a macro to derive schemas for structs and enums.

When deriving a schema, you have three basic choices:

## Atom

When declaring a schema as Atom, the schema type will just be Schema::Atom("typename"). This means that as long as the type name stays the same, the type is considered to be compatible. Note that this is the *local* type name. The schema macro can not figure out the canonical type name, and in any case doing so is out of scope for this simple crate.

Most rust primitive types are atoms.

## Structural

When declaring a schema as structural, all naming information will be purged. E.g. a struct with named fields will be considered identical to a tuple or product type, an enum with named fields will be considered identical to a sum type.

```rust
#[schema(Structural)]
struct Point {
    x: u64,
    y: u64,
}
```

has the same schema as `(u64, u64)`. Renaming the struct or the fields does not cause a schema change.

```rust
#[schema(Structural)]
enum Test {
    Case1(u64),
    Case2(&'static str)
}
```

has the same schema as `Result<u64, &'static str>`. Renaming the cases or the enum does not cause a schema change.

## Nominal

When declaring a schema as nominal, naming information will be kept. E.g. for a struct with named fields the names of the struct and the names of the fields will be included in the schema.

```rust
#[schema(Nominal)]
struct Point {
    x: f64,
    y: f64,
}
```

is a different schema than

```rust
#[schema(Nominal)]
struct Point {
    r: f64,
    psi: f64,
}
```

despite being compatible in terms of serialized representation. Use nominal if you want to attach meaning in addition to the constituent types.

The name defaults to the name of the Rust type. `#[schema(Nominal(name = "my_crate::Point"))]` sets it explicitly, for example to keep the schema when renaming the type, or to make a name unlikely to collide with other protocols.

For the request and response types of a protocol, use nominal: a request type is defined by its meaning, not just its shape. Use structural only for types where the names really carry no meaning.

The order of elements in a nominal or structural enum matters.

# Support for built in types

Basic rust types such as integers and floating point numbers are supported, as are rust collections such as BTreeSet/Map and HashSet/Map.

`PathBuf`, `Duration` and the `std::net` address types are atoms with their short name, such as `"PathBuf"` or `"SocketAddr"`.

Optional features add atoms for common types from other crates:

- `url`: `Url`
- `iroh-base`: `PublicKey` (which is `EndpointId`) as `"ed25519::PublicKey"`, and `EndpointAddr` and `RelayUrl` as `"iroh::EndpointAddr"` and `"iroh::RelayUrl"`

## Naming atoms

Name an atom after the encoding and meaning it promises, not after the Rust type that implements it. Two types with the same atom are promised to be interchangeable on the wire, and types that are not should have different atoms.

For example, `iroh_blobs::Hash` is a BLAKE3 hash in the same encoding as `blake3::Hash`, so its atom is `"blake3::Hash"`. An iroh `PublicKey` is an Ed25519 public key, so its atom is `"ed25519::PublicKey"`. An `EndpointAddr`, on the other hand, really is specific to iroh, so its atom is `"iroh::EndpointAddr"`. Types from the standard library keep their short names, such as `"PathBuf"`.

Atom names are global, so a name that only says `"Hash"` or `"Key"` is likely to mean different things in different places.

# Fields of other types

A field whose type does not implement `HasSchema`, for example a type from another crate, can name its schema as an atom:

```rust
#[schema(Nominal)]
struct Request {
    #[schema(atom = "blake3::Hash")]
    hash: iroh_blobs::Hash,
}
```

The field's schema is then the atom `"blake3::Hash"`, and the type does not need to implement `HasSchema`. As with any atom, keeping the serialized format of that type compatible is up to you.

# Recursive types

Schemas are trees, so deriving a schema for a recursive type would never end. Declare a recursive type as an atom instead, which promises its shape by name:

```rust
#[schema(Atom(name = "Tree"))]
struct Tree {
    children: Vec<Tree>,
}
```

This is in line with the rest of the crate: n0-schema checks the structure where that is cheap, and relies on named promises everywhere else.

# Schema evolution

For complex atom types that should be able to evolve, the best way is to introduce a version varint to the serde serialization. This isn't different to what you would do in serde without using n0-schema.

The pattern: wrap the public type (say `Color`) in a wire enum (`ColorWire`) declared as `#[schema(Atom(name = "Color"))]` so its schema hash is stable, with one variant per historical layout — `Version0(ColorV0)`, `Version1(Color)`, and so on. Old layouts get their own struct (`ColorV0`); the current layout reuses `Color` directly. New writes always emit the latest variant; older bytes still decode and get upgraded to the current shape via `From` impls. Adding a new variant ships a new wire format without changing the schema.

See [examples/atom_evolution.rs] for a runnable Color example.

For protocol enums, where each variant is a separate request/response type, use `#[serialize_stable]` (see [examples/protocol.rs]) or `#[serialize_service]` for [irpc] services (see [examples/service.rs]) — these expose a per-variant schema and hash so individual messages can evolve independently. For `serialize_service`, each variant's schema hash includes the request type plus the channel kind (none / oneshot / mpsc) and payload type of both `Rx` and `Tx`, so any change to the response shape — kind or type — flips the hash. The macro crate has no dependency on irpc; the generated code uses `irpc::Channels<S>` from the consuming crate.

Each case must have a distinct schema. Two cases with the same payload type would share a hash, so the first use of the enum panics.

## Wire format

A protocol enum is serialized as a tuple of the 32-byte schema hash of the case and its payload. With postcard, a tuple has no length prefix, so the first 32 bytes of each message are the hash. This format is stable.

## What changes the hash

Each case has its own hash, which depends only on the schema of its payload, and for `serialize_service` its channels. So:

| Change | Hash of the case |
| --- | --- |
| Rename a variant of the protocol enum | unchanged |
| Reorder the variants of the protocol enum | unchanged |
| Add or remove other variants | unchanged |
| Rename the payload type, or one of its fields (nominal) | changed |
| Change the type of a field | changed |
| Change the response type or channel kind (`serialize_service`) | changed |
| Rename an atom | changed |

Unlike a plain serde enum, where a request is identified by its position, the order of the variants does not matter. A peer that does not know a hash fails to decode only that message.

## With irpc

Put `#[rpc_requests]` above `#[serialize_service]`. The payload and response types need `HasSchema`, usually from `#[schema(Nominal)]`:

```rust
#[schema(Nominal)]
#[derive(Debug, Serialize, Deserialize)]
struct Get {
    key: String,
}

#[rpc_requests(message = StoreMessage)]
#[serialize_service(StoreProtocol)]
#[derive(Debug)]
enum StoreProtocol {
    #[rpc(tx = oneshot::Sender<Option<String>>)]
    Get(Get),
}
```

A server that should survive requests it does not know uses `Handler::skip_bad_requests(true)`. The client of such a request then gets an error with `remote_code() == Some(ErrorCode::RequestDecodeFailed)`, and the connection stays open, so the client can fall back to an older request.

# Combining protocols

Since each case of a `serialize_stable` or `serialize_service` enum is identified by its schema hash, not its position, the cases of several such enums never get confused. `#[serialize_union]` combines them into one enum, which serializes exactly like the inner value and deserializes by looking up the hash in each inner enum:

```rust
#[serialize_union]
#[derive(Debug)]
enum Combined {
    Store(Store),
    Admin(Admin),
}
```

Unions can contain other unions. If two inner enums have a case with the same schema, the first use of the union panics.

For [irpc] services, `#[combine_services]` also implements `irpc::Service` for the union, and generates `Combined::handler(store_tx, admin_tx)`, which returns an `irpc::rpc::Handler` that sends each request to the actor of its service. A server can then serve several services on one ALPN, while each client keeps using its own `Client<Store>` or `Client<Admin>`. See [examples/combined.rs].

# Asking a server what it supports

With the `meta` feature, `n0_schema::meta::MetaProtocol` is a small service that lists the cases of a server, with name, hash, schema text and doc comment of each. The doc comments of the variants are not part of the schema, so editing them does not change any hash. Add it to the union of a server, and answer it with `meta::serve`:

```rust
#[combine_services]
#[derive(Debug)]
enum Server {
    Meta(MetaProtocol),
    Store(StoreProtocol),
}

let handler = Server::handler(n0_schema::meta::serve::<Server>(), store_tx);
```

A client lists the cases with `ListCases`, and compares their hashes with `n0_schema::request_hash::<StoreProtocol, Get>()` to check whether the server knows a request before sending it. The requests of `MetaProtocol` never change.

[postcard-rpc]: https://github.com/jamesmunns/postcard-rpc
[BLAKE3]: https://github.com/BLAKE3-team/BLAKE3-specs
[irpc]: https://docs.rs/irpc
[n0pe]: https://github.com/n0-computer/n0pe
[examples/atom_evolution.rs]: examples/atom_evolution.rs
[examples/protocol.rs]: examples/protocol.rs
[examples/service.rs]: examples/service.rs
[examples/combined.rs]: examples/combined.rs
