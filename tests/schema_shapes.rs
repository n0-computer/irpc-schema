#![cfg(feature = "derive")]
#![allow(dead_code)]

use n0_schema::{schema, HasSchema, Named, Schema};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

macro_rules! shapes {
    ($module:ident, $mode:ident) => {
        mod $module {
            use super::*;

            #[schema($mode)]
            pub struct Unit;
            #[schema($mode)]
            pub struct EmptyTuple();
            #[schema($mode)]
            pub struct EmptyRecord {}
            #[schema($mode)]
            pub struct Tuple(u8);
            #[schema($mode)]
            pub struct Record {
                a: u8,
            }
            #[schema($mode)]
            pub struct Pair(u8, u16);
            #[schema($mode)]
            pub struct Fields {
                a: u8,
                b: u16,
            }

            #[schema($mode)]
            pub enum Empty {}
            #[schema($mode)]
            pub enum Single {
                Value,
            }
            #[schema($mode)]
            pub enum Variants {
                Unit,
                EmptyTuple(),
                EmptyRecord {},
                Tuple(u8),
                Record { a: u8 },
                Pair(u8, u16),
                Fields { a: u8, b: u16 },
            }
        }
    };
}

shapes!(structural, Structural);
shapes!(nominal, Nominal);

#[test]
fn structural_structs_match_tuples_at_every_cardinality() {
    assert_eq!(<()>::schema(), Schema::Product(vec![]));
    assert_eq!(<(u8,)>::schema(), Schema::Product(vec![u8::schema()]));
    assert_ne!(<(u8,)>::schema(), u8::schema());
    for actual in [
        structural::Unit::schema(),
        structural::EmptyTuple::schema(),
        structural::EmptyRecord::schema(),
    ] {
        assert_eq!(actual, <()>::schema());
    }
    for actual in [structural::Tuple::schema(), structural::Record::schema()] {
        assert_eq!(actual, <(u8,)>::schema());
    }
    for actual in [structural::Pair::schema(), structural::Fields::schema()] {
        assert_eq!(actual, <(u8, u16)>::schema());
    }
}

#[test]
fn structural_enums_are_sums_of_products_at_every_cardinality() {
    assert_eq!(structural::Empty::schema(), Schema::Sum(vec![]));
    assert_eq!(
        structural::Single::schema(),
        Schema::Sum(vec![<()>::schema()])
    );
    assert_eq!(
        structural::Variants::schema(),
        Schema::Sum(vec![
            <()>::schema(),
            <()>::schema(),
            <()>::schema(),
            <(u8,)>::schema(),
            <(u8,)>::schema(),
            <(u8, u16)>::schema(),
            <(u8, u16)>::schema(),
        ])
    );
}

#[test]
fn nominal_structs_retain_field_names_at_every_cardinality() {
    for (actual, name, fields) in [
        (nominal::Unit::schema(), "Unit", <()>::schema()),
        (nominal::EmptyTuple::schema(), "EmptyTuple", <()>::schema()),
        (
            nominal::EmptyRecord::schema(),
            "EmptyRecord",
            Schema::Struct(vec![]),
        ),
        (nominal::Tuple::schema(), "Tuple", <(u8,)>::schema()),
        (
            nominal::Record::schema(),
            "Record",
            Schema::Struct(vec![Named::new("a", u8::schema())]),
        ),
        (nominal::Pair::schema(), "Pair", <(u8, u16)>::schema()),
        (
            nominal::Fields::schema(),
            "Fields",
            Schema::Struct(vec![
                Named::new("a", u8::schema()),
                Named::new("b", u16::schema()),
            ]),
        ),
    ] {
        assert_eq!(actual, Schema::named(name, fields));
    }
}

#[test]
fn nominal_enums_retain_variant_names_at_every_cardinality() {
    assert_eq!(
        nominal::Empty::schema(),
        Schema::named("Empty", Schema::Enum(vec![]))
    );
    assert_eq!(
        nominal::Single::schema(),
        Schema::named(
            "Single",
            Schema::Enum(vec![Named::new("Value", <()>::schema())])
        )
    );
    assert_eq!(
        nominal::Variants::schema(),
        Schema::named(
            "Variants",
            Schema::Enum(vec![
                Named::new("Unit", <()>::schema()),
                Named::new("EmptyTuple", <()>::schema()),
                Named::new("EmptyRecord", Schema::Struct(vec![])),
                Named::new("Tuple", <(u8,)>::schema()),
                Named::new(
                    "Record",
                    Schema::Struct(vec![Named::new("a", u8::schema())])
                ),
                Named::new("Pair", <(u8, u16)>::schema()),
                Named::new(
                    "Fields",
                    Schema::Struct(vec![
                        Named::new("a", u8::schema()),
                        Named::new("b", u16::schema())
                    ])
                ),
            ])
        )
    );
}

#[schema(Structural)]
#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum ManualOption {
    None,
    Some(u64),
}

#[schema(Structural)]
#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum ManualResult {
    Ok(u64),
    Err(String),
}

#[schema(Structural)]
#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum ManualUnitOption {
    None,
    Some(()),
}

#[schema(Structural)]
#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum ManualNested {
    Ok(ManualOption),
    Err(()),
}

fn assert_equivalent<A, B>(a: A, b: B)
where
    A: HasSchema + Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
    B: HasSchema + Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    assert_eq!(A::schema(), B::schema());
    assert_eq!(A::schema().stable_hash(), B::schema().stable_hash());
    let a_bytes = postcard::to_allocvec(&a).unwrap();
    let b_bytes = postcard::to_allocvec(&b).unwrap();
    assert_eq!(a_bytes, b_bytes);
    assert_eq!(postcard::from_bytes::<A>(&b_bytes).unwrap(), a);
    assert_eq!(postcard::from_bytes::<B>(&a_bytes).unwrap(), b);
}

#[test]
fn option_and_result_match_manual_enums_in_schema_and_encoding() {
    assert_equivalent(None::<u64>, ManualOption::None);
    assert_equivalent(Some(300u64), ManualOption::Some(300));
    assert_equivalent(Ok::<u64, String>(300), ManualResult::Ok(300));
    assert_equivalent(
        Err::<u64, String>("error".into()),
        ManualResult::Err("error".into()),
    );
    assert_equivalent(None::<()>, ManualUnitOption::None);
    assert_equivalent(Some(()), ManualUnitOption::Some(()));
    assert_equivalent(
        Ok::<Option<u64>, ()>(None),
        ManualNested::Ok(ManualOption::None),
    );
    assert_equivalent(
        Ok::<Option<u64>, ()>(Some(300)),
        ManualNested::Ok(ManualOption::Some(300)),
    );
    assert_equivalent(Err::<Option<u64>, ()>(()), ManualNested::Err(()));
}

#[test]
fn empty_products_and_sums_keep_their_display_and_distinct_hashes() {
    for (schema, text) in [
        (Schema::Product(vec![]), "()"),
        (Schema::Struct(vec![]), "()"),
        (Schema::Sum(vec![]), "⊥"),
        (Schema::Enum(vec![]), "⊥"),
    ] {
        assert_eq!(schema.to_string(), text);
        assert_eq!(schema.pretty_print(2), format!("  {text}"));
        let bytes = postcard::to_allocvec(&schema).unwrap();
        assert_eq!(postcard::from_bytes::<Schema>(&bytes).unwrap(), schema);
    }
    assert_ne!(
        Schema::Product(vec![]).stable_hash(),
        Schema::Sum(vec![]).stable_hash()
    );
    assert_ne!(
        structural::Unit::schema().stable_hash(),
        structural::Single::schema().stable_hash()
    );
}
