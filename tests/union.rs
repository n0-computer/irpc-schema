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
fn overlapping_cases_panic() {
    fn assert_overlap_panic(f: impl FnOnce() + std::panic::UnwindSafe) {
        let panic = std::panic::catch_unwind(f).expect_err("overlapping cases must panic");
        let message = panic.downcast_ref::<String>().expect("panic message");
        assert!(message.contains("have the same schema hash"), "{message}");
    }

    let reader = Reader::Get(Get { key: "k".into() });
    let bytes = postcard::to_allocvec(&reader).unwrap();
    let overlapping = Overlapping::Reader(reader);
    assert_overlap_panic(|| {
        let _ = postcard::to_allocvec(&overlapping);
    });
    assert_overlap_panic(|| {
        let _ = postcard::from_bytes::<Overlapping>(&bytes);
    });
    // Validate the type even if the input is too short to read a hash.
    assert_overlap_panic(|| {
        let _ = postcard::from_bytes::<Overlapping>(&[]);
    });
    assert_overlap_panic(|| {
        let _ = <Overlapping as StableEnum>::case_schemas();
    });
}

/// An enum with two cases of the same type, so the same hash.
#[serialize_stable]
#[derive(Debug, Clone, PartialEq)]
enum Duplicate {
    First(Get),
    Second(Get),
}

#[test]
#[should_panic(expected = "have the same schema hash")]
fn duplicate_case_types_panic() {
    let _ = postcard::to_allocvec(&Duplicate::Second(Get { key: "k".into() }));
}

/// `Store` with doc comments.
#[serialize_stable]
#[derive(Debug, Clone, PartialEq)]
enum DocumentedStore {
    /// Reads a value.
    ///
    /// Returns nothing if the key is missing.
    Get(Get),
    Put(Put),
}

#[test]
fn docs_do_not_change_the_hash() {
    let plain: Vec<_> = Store::schemas().map(|(_, _, hash)| hash).collect();
    let documented: Vec<_> = DocumentedStore::schemas()
        .map(|(_, _, hash)| hash)
        .collect();
    assert_eq!(plain, documented);
    assert_eq!(
        <DocumentedStore as StableEnum>::case_docs(),
        [
            "Reads a value.\n\nReturns nothing if the key is missing.",
            ""
        ]
    );
    assert_eq!(
        <Combined as StableEnum>::case_docs().len(),
        Combined::schemas().count()
    );
}
