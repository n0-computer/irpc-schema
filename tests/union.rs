//! Serialization of unions of hash-discriminated enums.
use n0_schema::{schema, serialize_stable, serialize_union, StableEnum};
use serde::{Deserialize, Serialize};

#[schema(Nominal)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Get {
    key: String,
}

#[schema(Nominal)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Put {
    key: String,
    value: String,
}

#[schema(Nominal)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Ping {
    seq: u64,
}

#[schema(Nominal)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Shutdown {}

#[serialize_stable]
#[derive(Debug, Clone, PartialEq)]
enum Store {
    Get(Get),
    Put(Put),
}

#[serialize_stable]
#[derive(Debug, Clone, PartialEq)]
enum Health {
    Ping(Ping),
}

#[serialize_stable]
#[derive(Debug, Clone, PartialEq)]
enum Admin {
    Shutdown(Shutdown),
}

#[serialize_union]
#[derive(Debug, Clone, PartialEq)]
enum Combined {
    Store(Store),
    Health(Health),
    Admin(Admin),
}

/// A union can contain another union.
#[serialize_union]
#[derive(Debug, Clone, PartialEq)]
enum Nested {
    Inner(Combined),
    Admin2(Admin2),
}

#[serialize_stable]
#[derive(Debug, Clone, PartialEq)]
enum Admin2 {
    Restart(Restart),
}

#[schema(Nominal)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Restart {}

#[test]
fn union_has_the_wire_format_of_its_cases() {
    let store = Store::Put(Put {
        key: "k".into(),
        value: "v".into(),
    });
    let health = Health::Ping(Ping { seq: 7 });
    let admin = Admin::Shutdown(Shutdown {});
    for (bytes, expected) in [
        (
            postcard::to_allocvec(&store).unwrap(),
            Combined::Store(store.clone()),
        ),
        (
            postcard::to_allocvec(&health).unwrap(),
            Combined::Health(health.clone()),
        ),
        (
            postcard::to_allocvec(&admin).unwrap(),
            Combined::Admin(admin.clone()),
        ),
    ] {
        assert_eq!(postcard::to_allocvec(&expected).unwrap(), bytes);
        let decoded: Combined = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, expected);
    }
}

#[test]
fn nested_union_decodes_inner_cases() {
    let health = Health::Ping(Ping { seq: 1 });
    let bytes = postcard::to_allocvec(&health).unwrap();
    let decoded: Nested = postcard::from_bytes(&bytes).unwrap();
    assert_eq!(decoded, Nested::Inner(Combined::Health(health)));
    let restart = Admin2::Restart(Restart {});
    let bytes = postcard::to_allocvec(&restart).unwrap();
    let decoded: Nested = postcard::from_bytes(&bytes).unwrap();
    assert_eq!(decoded, Nested::Admin2(restart));
    assert_eq!(Nested::schemas().count(), 5);
}

#[test]
fn unknown_case_fails() {
    let restart = Admin2::Restart(Restart {});
    let bytes = postcard::to_allocvec(&restart).unwrap();
    assert!(postcard::from_bytes::<Combined>(&bytes).is_err());
}

/// An enum with a case that `Store` also has.
#[serialize_stable]
#[derive(Debug, Clone, PartialEq)]
enum Reader {
    Get(Get),
}

#[serialize_union]
#[derive(Debug, Clone, PartialEq)]
enum Overlapping {
    Store(Store),
    Reader(Reader),
}

#[test]
#[should_panic(expected = "have the same schema hash")]
fn overlapping_cases_panic() {
    let _ = <Overlapping as StableEnum>::case_schemas();
}
