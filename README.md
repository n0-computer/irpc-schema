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

When declaring a schema as structural, all naming information will be purged. E.g. a struct with named fields will be considered identital to a tuple or product type, an enum with named fields will be considered identical to a sum type.

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

The order of elements in a nominal or structural enum matters.

# Support for built in types

Basic rust types such as integers and floating point numbers are supported, as are rust collections such as BTreeSet/Map and HashSet/Map.

# Schema evolution

For complex atom types that should be able to evolve, the best way is to introduce a version varint to the serde serialization. This isn't different to what you would do in serde without using n0-schema.

The pattern: wrap the public type (say `Color`) in a wire enum (`ColorWire`) declared as `#[schema(Atom(name = "Color"))]` so its schema hash is stable, with one variant per historical layout — `Version0(ColorV0)`, `Version1(Color)`, and so on. Old layouts get their own struct (`ColorV0`); the current layout reuses `Color` directly. New writes always emit the latest variant; older bytes still decode and get upgraded to the current shape via `From` impls. Adding a new variant ships a new wire format without changing the schema.

See [examples/atom_evolution.rs] for a runnable Color example.

For protocol enums, where each variant is a separate request/response type, use `#[serialize_stable]` (see [examples/protocol.rs]) or `#[serialize_service]` for [irpc] services (see [examples/service.rs]) — these expose a per-variant schema and hash so individual messages can evolve independently. For `serialize_service`, each variant's schema hash includes the request type plus the channel kind (none / oneshot / mpsc) and payload type of both `Rx` and `Tx`, so any change to the response shape — kind or type — flips the hash. The macro crate has no dependency on irpc; the generated code uses `irpc::Channels<S>` from the consuming crate.

[postcard-rpc]: https://github.com/jamesmunns/postcard-rpc
[BLAKE3]: https://github.com/BLAKE3-team/BLAKE3-specs
[irpc]: https://docs.rs/irpc
[n0pe]: https://github.com/n0-computer/n0pe
[examples/atom_evolution.rs]: examples/atom_evolution.rs
[examples/protocol.rs]: examples/protocol.rs
[examples/service.rs]: examples/service.rs
