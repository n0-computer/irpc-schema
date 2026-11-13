#![allow(dead_code)]
use std::collections::{BTreeMap, BTreeSet};

use n0_schema::{HasSchema, Named, Schema};
use n0_schema_derive::{schema, serialize_stable};
use testresult::TestResult;

#[schema(Nominal)]
struct UnitStruct;

#[schema(Nominal)]
enum BottomEnum {}

#[schema(Nominal)]
enum SingleCaseEnum {
    Case1,
}

#[schema(Nominal)]
struct NominalTupleStruct(i32, String);

#[schema(Nominal)]
struct NominalStruct {
    id: i32,
    name: String,
}

#[schema(Nominal(name = "CustomName"))]
struct CustomNamedStruct {
    value: u32,
}

#[schema(Atom(name = "CustomAtom"))]
struct CustomAtomStruct;

#[schema(Nominal)]
enum NominalEnum {
    Tuple(i32, String),
    NominalTupleStruct(NominalTupleStruct),
    Record { id: i32, name: String },
    NominalStruct(NominalStruct),
    Unit,
    UnitStruct(UnitStruct),
    BottomEnum(BottomEnum),
    SingleCaseEnum(SingleCaseEnum),
    Seq(Vec<u64>),
    Set(BTreeSet<u64>),
    Map(BTreeMap<u64, u64>),
}

#[schema(Structural)]
enum StructuralEnum {
    Tuple(i32, String),
    NominalTupleStruct(NominalTupleStruct),
    Record { id: i32, name: String },
    NominalStruct(NominalStruct),
    Unit,
    UnitStruct(UnitStruct),
    BottomEnum(BottomEnum),
    SingleCaseEnum(SingleCaseEnum),
}

#[test]
fn test_pretty_print() {
    println!("{}", StructuralEnum::schema().pretty_print(0));
    println!("{}", NominalEnum::schema().pretty_print(0));
    println!("{}", UnitStruct::schema().pretty_print(0));
    println!("{}", BottomEnum::schema().pretty_print(0));
    println!("{}", SingleCaseEnum::schema().pretty_print(0));
    println!("{}", NominalTupleStruct::schema().pretty_print(0));
    println!("{}", NominalStruct::schema().pretty_print(0));
    println!("{}", NominalEnum::schema().pretty_print(0));
}

#[test]
fn test_unit_struct_schema() {
    assert_eq!(
        UnitStruct::schema(),
        Schema::named("UnitStruct", Schema::Unit)
    );
}

#[test]
fn test_bottom_enum_schema() {
    assert_eq!(
        BottomEnum::schema(),
        Schema::named("BottomEnum", Schema::Bottom)
    );
}

#[test]
fn test_custom_named_struct() {
    assert_eq!(
        CustomNamedStruct::schema(),
        Schema::named(
            "CustomName",
            Schema::Struct(vec![Named(
                "value".to_string(),
                Schema::Atom("u32".to_string())
            )])
        )
    );
}

#[test]
fn test_custom_atom_struct() {
    assert_eq!(
        CustomAtomStruct::schema(),
        Schema::Atom("CustomAtom".to_string())
    );
}

#[test]
fn test_nominal_enum() {
    println!("NominalEnum: {}", NominalEnum::schema());
    println!("{}", NominalEnum::schema().pretty_print(0));
}

#[test]
fn test_structural_enum() {
    println!("StructuralEnum: {}", StructuralEnum::schema());
    println!("{}", StructuralEnum::schema().pretty_print(0));
}

#[test]
fn test_enum_cases() {
    let schema = NominalEnum::schema();
    let Schema::Named(name) = schema else {
        panic!("Expected Named");
    };
    let Schema::Enum(cases) = name.1 else {
        panic!("Expected Enum");
    };
    for Named(name, value) in cases {
        println!("{}: {}", name, value.stable_hash());
    }
}

#[test]
fn test_serialize_schema() -> TestResult<()> {
    #[derive(Debug, Eq, PartialEq)]
    #[serialize_stable]
    enum Test {
        Foo(u32),
        Bar(String),
    }

    let v = Test::Foo(1);
    let v_bytes = postcard::to_allocvec(&v)?;
    let mut expected = u32::schema().stable_hash().as_bytes().to_vec();
    expected.extend_from_slice(&postcard::to_allocvec(&1u32)?);
    assert_eq!(v_bytes, expected);
    let v_out: Test = postcard::from_bytes(&v_bytes)?;
    assert_eq!(v, v_out);
    Ok(())
}

/// A type from "another crate" without `HasSchema`.
#[derive(Debug)]
struct Foreign([u8; 32]);

#[schema(Nominal)]
struct WithForeignField {
    #[schema(atom = "Foreign")]
    foreign: Foreign,
    count: u64,
}

#[schema(Nominal)]
struct WithForeignTuple(#[schema(atom = "Foreign")] Foreign, u64);

#[schema(Nominal)]
enum WithForeignVariants {
    Record {
        #[schema(atom = "Foreign")]
        foreign: Foreign,
    },
    Tuple(#[schema(atom = "Foreign")] Foreign, u64),
}

#[schema(Structural)]
struct StructuralWithForeign {
    #[schema(atom = "Foreign")]
    foreign: Foreign,
    count: u64,
}

#[test]
fn test_field_atom_attribute() {
    let foreign = || Schema::Atom("Foreign".to_string());
    assert_eq!(
        WithForeignField::schema(),
        Schema::Named(Box::new(Named(
            "WithForeignField".to_string(),
            Schema::Struct(vec![
                Named("foreign".to_string(), foreign()),
                Named("count".to_string(), u64::schema()),
            ])
        )))
    );
    assert_eq!(
        WithForeignTuple::schema(),
        Schema::Named(Box::new(Named(
            "WithForeignTuple".to_string(),
            Schema::Product(vec![foreign(), u64::schema()])
        )))
    );
    assert_eq!(
        StructuralWithForeign::schema(),
        Schema::Product(vec![foreign(), u64::schema()])
    );
    // The attribute is removed from the emitted type, and the enum schema uses it.
    let text = WithForeignVariants::schema().to_string();
    assert!(text.contains("Foreign"), "{text}");
    assert!(!text.contains("u8"), "{text}");
}

#[test]
fn test_std_atoms() {
    let atom = |name: &str| Schema::Atom(name.to_string());
    assert_eq!(std::path::PathBuf::schema(), atom("PathBuf"));
    assert_eq!(std::time::Duration::schema(), atom("Duration"));
    assert_eq!(std::net::IpAddr::schema(), atom("IpAddr"));
    assert_eq!(std::net::SocketAddr::schema(), atom("SocketAddr"));
    assert_eq!(std::net::SocketAddrV4::schema(), atom("SocketAddrV4"));
    assert_eq!(std::net::SocketAddrV6::schema(), atom("SocketAddrV6"));
}

#[cfg(feature = "url")]
#[test]
fn test_url_atom() {
    assert_eq!(url::Url::schema(), Schema::Atom("Url".to_string()));
}

#[cfg(feature = "iroh-base")]
#[test]
fn test_iroh_base_atoms() {
    assert_eq!(
        iroh_base::PublicKey::schema(),
        Schema::Atom("PublicKey".to_string())
    );
    assert_eq!(
        iroh_base::EndpointAddr::schema(),
        Schema::Atom("EndpointAddr".to_string())
    );
}
