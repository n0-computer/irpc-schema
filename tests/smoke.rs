#![allow(dead_code)]
use std::collections::{BTreeMap, BTreeSet};

use irpc_schema::{HasSchema, Named, Schema};
use irpc_schema_derive::{schema, serialize_stable};
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

mod two_stable_enums_in_one_module {
    use irpc_schema_derive::serialize_stable;

    #[serialize_stable]
    enum First {
        Foo(u32),
    }

    #[serialize_stable]
    enum Second {
        Foo(u32),
    }

    #[test]
    fn test_two_stable_enums_in_one_module_have_the_same_hash() {
        let (_, _, first) = First::schemas().next().unwrap();
        let (_, _, second) = Second::schemas().next().unwrap();
        assert_eq!(first, second);
    }
}

mod flatten {
    use irpc_schema::StableEnum;
    use irpc_schema_derive::serialize_stable;
    use testresult::TestResult;

    #[derive(Debug, PartialEq)]
    #[serialize_stable]
    enum Numbers {
        Int(u32),
    }

    #[derive(Debug, PartialEq)]
    #[serialize_stable]
    enum Words {
        Word(String),
    }

    #[derive(Debug, PartialEq)]
    #[serialize_stable]
    enum Combined {
        Flag(bool),
        #[flatten]
        Numbers(Numbers),
        #[flatten]
        Words(Words),
    }

    #[derive(Debug, PartialEq)]
    #[serialize_stable]
    enum MoreNumbers {
        Int(u32),
    }

    #[derive(Debug, PartialEq)]
    #[serialize_stable]
    enum Ambiguous {
        #[flatten]
        Numbers(Numbers),
        #[flatten]
        MoreNumbers(MoreNumbers),
    }

    #[test]
    fn test_flatten_serializes_like_the_inner_enum() -> TestResult<()> {
        let inner = postcard::to_allocvec(&Words::Word("hi".to_string()))?;
        let outer = postcard::to_allocvec(&Combined::Words(Words::Word("hi".to_string())))?;
        assert_eq!(inner, outer);
        Ok(())
    }

    #[test]
    fn test_flatten_deserializes_inner_enum_bytes() -> TestResult<()> {
        let bytes = postcard::to_allocvec(&Numbers::Int(7))?;
        let value: Combined = postcard::from_bytes(&bytes)?;
        assert_eq!(value, Combined::Numbers(Numbers::Int(7)));
        let bytes = postcard::to_allocvec(&Words::Word("hi".to_string()))?;
        let value: Combined = postcard::from_bytes(&bytes)?;
        assert_eq!(value, Combined::Words(Words::Word("hi".to_string())));
        Ok(())
    }

    #[test]
    fn test_flatten_keeps_payload_variants() -> TestResult<()> {
        let bytes = postcard::to_allocvec(&Combined::Flag(true))?;
        let value: Combined = postcard::from_bytes(&bytes)?;
        assert_eq!(value, Combined::Flag(true));
        Ok(())
    }

    #[test]
    fn test_flatten_rejects_unknown_hash() -> TestResult<()> {
        let bytes = postcard::to_allocvec(&Combined::Flag(true))?;
        assert!(postcard::from_bytes::<Numbers>(&bytes).is_err());
        Ok(())
    }

    #[test]
    fn test_flatten_schemas_lists_inner_variants() {
        let names: Vec<_> = Combined::schemas().map(|(name, _, _)| name).collect();
        assert_eq!(names, ["Flag", "Int", "Word"]);
        let hashes: Vec<_> = <Combined as StableEnum>::schemas()
            .map(|(_, _, hash)| hash)
            .collect();
        let (_, _, int) = Numbers::schemas().next().unwrap();
        assert_eq!(hashes[1], int);
    }

    #[derive(Debug, PartialEq)]
    #[serialize_stable]
    enum Duplicate {
        First(u32),
        Second(u32),
    }

    #[test]
    fn test_flatten_gives_a_shared_hash_to_the_first_variant() -> TestResult<()> {
        let bytes = postcard::to_allocvec(&MoreNumbers::Int(7))?;
        let value: Ambiguous = postcard::from_bytes(&bytes)?;
        assert_eq!(value, Ambiguous::Numbers(Numbers::Int(7)));
        Ok(())
    }

    #[test]
    fn test_flatten_lists_a_shared_hash_once() {
        let names: Vec<_> = Ambiguous::schemas().map(|(name, _, _)| name).collect();
        assert_eq!(names, ["Int"]);
    }

    #[test]
    #[should_panic(expected = "First and Second in Duplicate have the same schema hash")]
    fn test_payload_variants_with_the_same_hash_panic() {
        let _ = Duplicate::schemas().count();
    }
}

mod derive {
    use irpc_schema::HasSchema;

    #[derive(HasSchema)]
    struct Derived {
        id: u32,
        name: String,
    }

    mod attribute {
        #[irpc_schema::schema(Nominal)]
        pub struct Derived {
            id: u32,
            name: String,
        }
    }

    #[test]
    fn test_derive_matches_the_nominal_attribute() {
        assert_eq!(Derived::schema(), attribute::Derived::schema());
    }
}

mod generics_and_common_types {
    use std::{marker::PhantomData, time::Duration};

    use irpc_schema::{schema, HasSchema, Schema};

    #[schema(Nominal)]
    struct Wrapper<T> {
        value: T,
    }

    #[derive(HasSchema)]
    struct Derived<T> {
        value: T,
    }

    #[test]
    fn test_generic_schema_depends_on_the_type_argument() {
        assert_ne!(Wrapper::<u32>::schema(), Wrapper::<String>::schema());
        assert_ne!(Derived::<u32>::schema(), Derived::<u8>::schema());
    }

    #[test]
    fn test_array_has_the_schema_of_a_tuple() {
        assert_eq!(<[u8; 2]>::schema(), <(u8, u8)>::schema());
    }

    #[test]
    fn test_common_types_have_schemas() {
        assert_eq!(
            Duration::schema(),
            Schema::Atom("std::time::Duration".to_string())
        );
        assert_eq!(PhantomData::<String>::schema(), Schema::Unit);
        assert_eq!(
            <(u8, u16, u32, u64)>::schema(),
            Schema::Product(vec![
                u8::schema(),
                u16::schema(),
                u32::schema(),
                u64::schema()
            ])
        );
    }
}

mod field_atoms {
    use irpc_schema::{schema, HasSchema, Named, Schema};

    /// Stands in for a type from another crate, without a schema.
    struct Foreign;

    #[derive(HasSchema)]
    struct Derived {
        #[schema(atom = "other::Foreign")]
        foreign: Foreign,
        count: u32,
    }

    #[schema(Nominal)]
    struct Attributed {
        #[schema(atom = "other::Foreign")]
        foreign: Foreign,
    }

    #[test]
    fn test_field_atom_replaces_the_schema_of_the_field() {
        let expected = Schema::named(
            "Derived",
            Schema::Struct(vec![
                Named::new("foreign", Schema::Atom("other::Foreign".to_string())),
                Named::new("count", u32::schema()),
            ]),
        );
        assert_eq!(Derived::schema(), expected);
        let Schema::Named(named) = Attributed::schema() else {
            panic!("a nominal schema is named");
        };
        let Schema::Struct(fields) = named.1 else {
            panic!("a struct with named fields has a struct schema");
        };
        assert_eq!(fields[0].1, Schema::Atom("other::Foreign".to_string()));
    }
}

mod recursion {
    use irpc_schema::{HasSchema, Named, Schema};

    #[derive(HasSchema)]
    enum Op {
        Get(u32),
        Txn(Txn),
    }

    #[derive(HasSchema)]
    struct Txn {
        ops: Vec<Op>,
    }

    #[test]
    fn test_recursive_type_refers_to_itself() {
        let txn = Schema::named(
            "Txn",
            Schema::Struct(vec![Named::new(
                "ops",
                Schema::Seq(Box::new(Schema::Ref("Op".to_string()))),
            )]),
        );
        let expected = Schema::named(
            "Op",
            Schema::Enum(vec![
                Named::new("Get", Schema::Product(vec![u32::schema()])),
                Named::new("Txn", Schema::Product(vec![txn])),
            ]),
        );
        assert_eq!(Op::schema(), expected);
    }
}
