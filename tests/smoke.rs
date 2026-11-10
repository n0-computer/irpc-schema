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
