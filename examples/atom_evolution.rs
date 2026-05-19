//! Evolving a complex atom type without changing its schema hash.
//!
//! [`Color`] is the ergonomic in-memory type. [`ColorWire`] is the wire-level
//! enum carrying one variant per historical layout, with the old layout kept
//! around as its own struct [`ColorV0`]. The schema is the stable atom
//! `"Color"`, so adding a new variant ships a new wire format without
//! changing the schema hash.
//!
//! Use [`ColorWire`] in protocol/service definitions; convert to and from
//! [`Color`] at the edges via the provided `From` impls.

use anyhow::Result;
use n0_schema::{schema, HasSchema};
use serde::{Deserialize, Serialize};

/// Current Color layout: RGBA.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// Original Color layout: RGB only. Alpha was implicitly fully opaque.
/// Kept around so old wire bytes still decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorV0 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl From<ColorV0> for Color {
    fn from(c: ColorV0) -> Self {
        Color {
            r: c.r,
            g: c.g,
            b: c.b,
            a: 0xFF,
        }
    }
}

/// On-the-wire representation of [`Color`]. Schema is the stable atom
/// `"Color"`; adding a new variant does not change the schema hash.
#[schema(Atom(name = "Color"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorWire {
    Version0(ColorV0),
    Version1(Color),
}

impl From<Color> for ColorWire {
    fn from(c: Color) -> Self {
        ColorWire::Version1(c)
    }
}

impl From<ColorWire> for Color {
    fn from(repr: ColorWire) -> Self {
        match repr {
            ColorWire::Version0(v0) => v0.into(),
            ColorWire::Version1(v1) => v1,
        }
    }
}

fn main() -> Result<()> {
    println!("schema: {}", ColorWire::schema().pretty_print(0));
    println!(
        "hash:   {}",
        hex::encode(ColorWire::schema().stable_hash().as_bytes())
    );

    // Round-trip the current Color through the wire enum. New writes go out
    // as the latest variant.
    let current = Color {
        r: 0x11,
        g: 0x22,
        b: 0x33,
        a: 0x80,
    };
    let bytes = postcard::to_allocvec(&ColorWire::from(current))?;
    let decoded: Color = postcard::from_bytes::<ColorWire>(&bytes)?.into();
    assert_eq!(current, decoded);

    // Backward compatibility: simulate bytes from an old peer that only knew
    // the v0 layout. Decoding into Color upgrades it to the current shape.
    let legacy = ColorWire::Version0(ColorV0 {
        r: 0x11,
        g: 0x22,
        b: 0x33,
    });
    let legacy_bytes = postcard::to_allocvec(&legacy)?;
    let upgraded: Color = postcard::from_bytes::<ColorWire>(&legacy_bytes)?.into();
    assert_eq!(
        upgraded,
        Color {
            r: 0x11,
            g: 0x22,
            b: 0x33,
            a: 0xFF
        }
    );
    println!("legacy v0 decoded as: {:?}", upgraded);

    Ok(())
}
