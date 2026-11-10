extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields, ItemEnum, Meta};

// The attribute macro for schema generation
#[proc_macro_attribute]
pub fn schema(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let name = &input.ident;

    // Parse the attribute to extract schema type and optional name
    let attr_meta = parse_macro_input!(attr as Meta);
    let (schema_type, explicit_name) = match attr_meta {
        Meta::Path(path) => {
            let schema_type = path.get_ident().unwrap().to_string();
            (schema_type, None)
        }
        Meta::List(list) => {
            let schema_type = list.path.get_ident().unwrap().to_string();
            let mut explicit_name = None;

            // Parse the nested meta items
            for nested in list.nested.iter() {
                match nested {
                    syn::NestedMeta::Meta(Meta::NameValue(nv)) if nv.path.is_ident("name") => {
                        if let syn::Lit::Str(lit_str) = &nv.lit {
                            explicit_name = Some(lit_str.value());
                        } else {
                            panic!("Expected string literal for name parameter");
                        }
                    }
                    _ => panic!("Unsupported parameter in schema attribute"),
                }
            }

            (schema_type, explicit_name)
        }
        _ => panic!("Unsupported attribute format"),
    };

    let schema_impl = match schema_type.as_str() {
        "Atom" => generate_atom_schema(name, explicit_name.as_deref()),
        "Structural" => generate_structural_schema(&input.data),
        "Nominal" => generate_nominal_schema(name, &input.data, explicit_name.as_deref()),
        _ => panic!("Unsupported schema type"),
    };

    let expanded = quote! {
        #input

        impl ::n0_schema::HasSchema for #name {
            fn schema() -> ::n0_schema::Schema {
                #schema_impl
            }
        }
    };

    TokenStream::from(expanded)
}

// Generates an Atom schema (just the type name)
fn generate_atom_schema(
    name: &syn::Ident,
    explicit_name: Option<&str>,
) -> proc_macro2::TokenStream {
    let type_name = match explicit_name {
        Some(name) => name.to_string(),
        None => name.to_string(),
    };
    quote! {
        ::n0_schema::Schema::Atom(#type_name.to_string())
    }
}

// Generates a Structural schema (tuples or unnamed structs)
fn generate_structural_schema(data: &syn::Data) -> proc_macro2::TokenStream {
    match data {
        Data::Struct(data_struct) => match &data_struct.fields {
            Fields::Named(fields) => {
                let types: Vec<proc_macro2::TokenStream> = fields
                    .named
                    .iter()
                    .map(|f| {
                        let ty = &f.ty;
                        quote! {
                            <#ty as ::n0_schema::HasSchema>::schema()
                        }
                    })
                    .collect();
                if types.is_empty() {
                    quote! {
                        ::n0_schema::Schema::Unit
                    }
                } else {
                    quote! {
                        ::n0_schema::Schema::Product(vec![#(#types),*])
                    }
                }
            }
            Fields::Unnamed(fields) => {
                let types: Vec<proc_macro2::TokenStream> = fields
                    .unnamed
                    .iter()
                    .map(|f| {
                        let ty = &f.ty;
                        quote! {
                            <#ty as ::n0_schema::HasSchema>::schema()
                        }
                    })
                    .collect();
                if types.is_empty() {
                    quote! {
                        ::n0_schema::Schema::Unit
                    }
                } else {
                    quote! {
                        ::n0_schema::Schema::Product(vec![#(#types),*])
                    }
                }
            }
            Fields::Unit => quote! {
                ::n0_schema::Schema::Unit
            },
        },
        Data::Enum(data_enum) => {
            let variant_schemas: Vec<proc_macro2::TokenStream> = data_enum
                .variants
                .iter()
                .map(|v| {
                    let variant_fields = match &v.fields {
                        Fields::Named(fields) => fields
                            .named
                            .iter()
                            .map(|f| {
                                let ty = &f.ty;
                                quote! {
                                    <#ty as ::n0_schema::HasSchema>::schema()
                                }
                            })
                            .collect(),
                        Fields::Unnamed(fields) => fields
                            .unnamed
                            .iter()
                            .map(|f| {
                                let ty = &f.ty;
                                quote! {
                                    <#ty as ::n0_schema::HasSchema>::schema()
                                }
                            })
                            .collect(),
                        Fields::Unit => vec![],
                    };
                    if variant_fields.is_empty() {
                        quote! {
                            ::n0_schema::Schema::Unit
                        }
                    } else {
                        quote! {
                            ::n0_schema::Schema::Product(vec![#(#variant_fields),*])
                        }
                    }
                })
                .collect();
            if variant_schemas.is_empty() {
                return quote! {
                    ::n0_schema::Schema::Bottom
                };
            }
            quote! {
                ::n0_schema::Schema::Sum(vec![#(#variant_schemas),*])
            }
        }
        _ => panic!("Unsupported type for Structural schema"),
    }
}

// Generates a Nominal schema (Struct or Enum with names)
fn generate_nominal_schema(
    name: &syn::Ident,
    data: &syn::Data,
    explicit_name: Option<&str>,
) -> proc_macro2::TokenStream {
    let name_text = explicit_name.unwrap_or(&name.to_string()).to_string();
    match data {
        Data::Struct(data_struct) => match &data_struct.fields {
            Fields::Named(fields) => {
                let field_schemas: Vec<proc_macro2::TokenStream> = fields
                    .named
                    .iter()
                    .map(|f| {
                        let field_name = f.ident.as_ref().unwrap().to_string();
                        let field_type = &f.ty;
                        quote! {
                            ::n0_schema::Named(#field_name.to_string(), <#field_type as ::n0_schema::HasSchema>::schema())
                        }
                    })
                    .collect();
                let schema = if field_schemas.is_empty() {
                    quote! { ::n0_schema::Schema::Unit }
                } else {
                    quote! { ::n0_schema::Schema::Struct(vec![#(#field_schemas),*]) }
                };
                quote! {
                    ::n0_schema::Schema::Named(
                        Box::new(::n0_schema::Named(#name_text.to_string(), #schema))
                    )
                }
            }
            Fields::Unnamed(fields) => {
                let field_schemas: Vec<proc_macro2::TokenStream> = fields
                    .unnamed
                    .iter()
                    .map(|f| {
                        let field_type = &f.ty;
                        quote! {
                            <#field_type as ::n0_schema::HasSchema>::schema()
                        }
                    })
                    .collect();
                let schema = if field_schemas.is_empty() {
                    quote! { ::n0_schema::Schema::Unit }
                } else {
                    quote! { ::n0_schema::Schema::Product(vec![#(#field_schemas),*]) }
                };
                quote! {
                    ::n0_schema::Schema::Named(
                        Box::new(::n0_schema::Named(#name_text.to_string(), #schema))
                    )
                }
            }
            Fields::Unit => quote! {
                ::n0_schema::Schema::Named(
                    Box::new(::n0_schema::Named(#name_text.to_string(), ::n0_schema::Schema::Unit))
                )
            },
        },
        Data::Enum(data_enum) => {
            let variants: Vec<proc_macro2::TokenStream> = data_enum
                .variants
                .iter()
                .map(|v| {
                    let variant_name = &v.ident;
                    let variant_name_text = variant_name.to_string();
                    match &v.fields {
                        Fields::Named(fields) => {
                            let named = fields
                                .named
                                .iter()
                                .map(|f| {
                                    let field_type = &f.ty;
                                    let field_name = f.ident.as_ref().unwrap().to_string();
                                    quote! {
                                        ::n0_schema::Named(#field_name.to_string(),<#field_type as ::n0_schema::HasSchema>::schema())
                                    }
                                })
                                .collect::<Vec<_>>();
                            let schema_type = if named.is_empty() {
                                quote! { ::n0_schema::Schema::Unit }
                            } else if named.len() == 1 {
                                quote! { ::n0_schema::Schema::Struct(vec![#(#named),*]) }
                            } else {
                                quote! { ::n0_schema::Schema::Enum(vec![#(#named),*]) }
                            };
                            quote! {
                                ::n0_schema::Named(
                                    #variant_name_text.to_string(),
                                    #schema_type
                                )
                            }
                        }
                        Fields::Unnamed(fields) => {
                            let unnamed = fields
                                .unnamed
                                .iter()
                                .map(|f| {
                                    let field_type = &f.ty;
                                    quote! {
                                        <#field_type as ::n0_schema::HasSchema>::schema()
                                    }
                                })
                                .collect::<Vec<_>>();
                            let schema_type = if unnamed.is_empty() {
                                quote! { ::n0_schema::Schema::Unit }
                            } else if unnamed.len() == 1 {
                                quote! { ::n0_schema::Schema::Product(vec![#(#unnamed),*]) }
                            } else {
                                quote! { ::n0_schema::Schema::Sum(vec![#(#unnamed),*]) }
                            };
                            quote! {
                                ::n0_schema::Named(
                                    #variant_name_text.to_string(),
                                    #schema_type
                                )
                            }
                        }
                        Fields::Unit => {
                            quote! {
                                ::n0_schema::Named(
                                    #variant_name_text.to_string(),
                                    ::n0_schema::Schema::Unit
                                )
                            }
                        }
                    }
                })
                .collect::<Vec<_>>();

            let schema = if variants.is_empty() {
                quote! { ::n0_schema::Schema::Bottom }
            } else if variants.len() == 1 {
                quote! { ::n0_schema::Schema::Struct(vec![#(#variants),*]) }
            } else {
                quote! { ::n0_schema::Schema::Enum(vec![#(#variants),*]) }
            };
            quote! {
                ::n0_schema::Schema::Named(
                    Box::new(::n0_schema::Named(#name_text.to_string(), #schema))
                )
            }
        }
        _ => panic!("Unsupported type for Nominal schema"),
    }
}

/// Implements stable serialization and deserialization for an enum with
/// a number of distinct variants.
///
/// Each variant must have a single unnamed field of distinct type. Each type
/// must implement `HasSchema`.
#[proc_macro_attribute]
pub fn serialize_stable(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let input = parse_macro_input!(item as ItemEnum);

    // Get the original enum
    let original_enum = input.clone();

    // Get the name of the enum
    let enum_name = &input.ident;

    // Generate names for our hash struct
    let schema_struct_name = syn::Ident::new(&format!("{}Schemas", enum_name), enum_name.span());
    let schema_struct_static_name =
        syn::Ident::new(&format!("__{}_SCHEMAS", enum_name), enum_name.span());

    // Collect all variants
    let variants = &input.variants;

    // Make sure all variants have a single unnamed field
    for variant in variants {
        match &variant.fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                // This is good - a single unnamed field
            }
            _ => panic!("HashDiscriminator only supports variants with a single unnamed field"),
        }
    }

    // Collect all variant names and their field types
    let mut variant_names = Vec::new();
    let mut field_types = Vec::new();

    for variant in variants {
        let variant_name = &variant.ident;
        variant_names.push(variant_name);

        let field_type = match &variant.fields {
            Fields::Unnamed(fields) => &fields.unnamed.first().unwrap().ty,
            _ => unreachable!(), // We've already checked this above
        };

        field_types.push(field_type);
    }

    // Define fields for our SchemaHashes struct
    let schema_struct_fields = variant_names.iter().map(|variant_name| {
        quote! { pub #variant_name: ::n0_schema::SchemaAndHash }
    });

    // Generate initialization for our SchemaHashes struct
    let schema_struct_inits =
        variant_names
            .iter()
            .zip(field_types.iter())
            .map(|(variant_name, field_type)| {
                quote! {
                    #variant_name: ::n0_schema::SchemaAndHash::from(<#field_type as ::n0_schema::HasSchema>::schema())
                }
            });

    let schema_struct_to_tuples = variant_names.iter().map(|variant_name| {
        let ident = variant_name.to_string();
        quote! {
            (#ident, &schema_struct_value.#variant_name.schema, schema_struct_value.#variant_name.hash)
        }
    });

    // Generate serialization arms using the static hashes
    let serialize_arms = variant_names.iter().map(|variant_name| {
        quote! {
            #enum_name::#variant_name(payload) => {
                let hash = schema_struct_value.#variant_name.hash;

                let mut tup = serializer.serialize_tuple(2)?;
                tup.serialize_element(&hash)?;
                tup.serialize_element(payload)?;
                tup.end()
            }
        }
    });

    // Generate deserialization branches using the static hashes
    let deserialize_branches =
        variant_names
            .iter()
            .zip(field_types.iter())
            .map(|(variant_name, field_type)| {
                quote! {
                    if &hash_bytes == &schema_struct_value.#variant_name.hash {
                        let payload = seq.next_element::<#field_type>()?.ok_or_else(||
                            serde::de::Error::custom("missing payload"))?;
                        return Ok(#enum_name::#variant_name(payload));
                    }
                }
            });

    // Generate the implementation
    let generated_impls = quote! {
        // The original enum definition
        #original_enum

        // Define a struct to hold the schema hashes
        #[allow(non_snake_case)]
        #[derive(Debug)]
        struct #schema_struct_name {
            #(#schema_struct_fields),*
        }

        // Create a static instance of our hashes using std::sync::OnceLock
        use std::sync::OnceLock;
        #[allow(non_upper_case_globals)]
        static #schema_struct_static_name: OnceLock<#schema_struct_name> = OnceLock::new();

        impl #schema_struct_name {
            // Create a new instance with all the hashes computed
            fn new() -> Self {
                Self {
                    #(#schema_struct_inits),*
                }
            }

            // Static accessor function to get or initialize the global instance
            fn get() -> &'static Self {
                #schema_struct_static_name.get_or_init(|| Self::new())
            }
        }

        impl #enum_name {
            pub fn schemas() -> impl ::std::iter::Iterator<Item = (&'static str, &'static ::n0_schema::Schema, [u8; 32])> {
                let schema_struct_value = #schema_struct_name::get();
                [#(#schema_struct_to_tuples),*].into_iter()
            }
        }

        // Implementation of serde::Serialize for the enum
        impl serde::Serialize for #enum_name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                use serde::ser::SerializeTuple;
                let schema_struct_value = #schema_struct_name::get();

                match self {
                    #(#serialize_arms),*
                }
            }
        }

        // Implementation of serde::Deserialize for the enum with visitor inside
        impl<'de> serde::Deserialize<'de> for #enum_name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                // Define the visitor struct inside the deserialize implementation
                struct Visitor;

                impl<'de> serde::de::Visitor<'de> for Visitor {
                    type Value = #enum_name;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                        formatter.write_str("a tuple with a hash discriminator and payload")
                    }

                    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
                    where
                        A: serde::de::SeqAccess<'de>,
                    {
                        // Deserialize the hash discriminator (first element)
                        let hash_bytes = seq.next_element::<[u8; 32]>()?.ok_or_else(||
                            serde::de::Error::custom("missing hash"))?;

                        // Get the schema hashes
                        let schema_struct_value = #schema_struct_name::get();

                        // Check against our static hashes
                        #(#deserialize_branches)*

                        // If none matched, return an error
                        Err(serde::de::Error::custom("unknown discriminator"))
                    }
                }

                // Use the locally-defined visitor
                deserializer.deserialize_tuple(2, Visitor)
            }
        }
    };

    // Return the generated code
    TokenStream::from(generated_impls)
}

/// This is identical to `serialize_stable`, but for a specific service.
///
/// The schema hashes for each variant will include not just the message type itself,
/// but also the kind (none, oneshot, spsc) and type of tx and rx channels it uses.
///
/// That way any change to channel kinds and types will result in a different hash,
/// just like a change to the message type itself.
///
/// Usage:
/// ```rust
/// #[serialize_service(MyService)]
/// enum MyServiceProto {}
/// ```
///
/// This macro requires that `irpc::Channels` is implemented for the given service type
/// for each variant of the enum. It also requires that HasSchema is implemented for
/// all channlels payload types.
#[proc_macro_attribute]
pub fn serialize_service(attr: TokenStream, item: TokenStream) -> TokenStream {
    // Service for which this macro is applied
    let service = parse_macro_input!(attr as syn::Ident);

    // Parse the input tokens into a syntax tree
    let input = parse_macro_input!(item as ItemEnum);

    // Get the original enum
    let original_enum = input.clone();

    // Get the name of the enum
    let enum_name = &input.ident;

    // Generate names for our hash struct
    let schema_struct_name = syn::Ident::new(&format!("{}Schemas", enum_name), enum_name.span());
    let schema_struct_static_name =
        syn::Ident::new(&format!("__{}_SCHEMAS", enum_name), enum_name.span());

    // Collect all variants
    let variants = &input.variants;

    // Make sure all variants have a single unnamed field
    for variant in variants {
        match &variant.fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                // This is good - a single unnamed field
            }
            _ => panic!("HashDiscriminator only supports variants with a single unnamed field"),
        }
    }

    // Collect all variant names and their field types
    let mut variant_names = Vec::new();
    let mut field_types = Vec::new();

    for variant in variants {
        let variant_name = &variant.ident;
        variant_names.push(variant_name);

        let field_type = match &variant.fields {
            Fields::Unnamed(fields) => &fields.unnamed.first().unwrap().ty,
            _ => unreachable!(), // We've already checked this above
        };

        field_types.push(field_type);
    }

    // Define fields for our SchemaHashes struct
    let schema_struct_fields = variant_names.iter().map(|variant_name| {
        quote! { pub #variant_name: ::n0_schema::SchemaAndHash }
    });

    // Generate initialization for our SchemaHashes struct
    let schema_struct_inits =
        variant_names
            .iter()
            .zip(field_types.iter())
            .map(|(variant_name, field_type)| {
                quote! {
                    #variant_name: ::n0_schema::SchemaAndHash::from(<#field_type as ::n0_schema::ChannelsSchema<#service>>::schema())
                }
            });

    let schema_struct_to_tuples = variant_names.iter().map(|variant_name| {
        let ident = variant_name.to_string();
        quote! {
            (#ident, &schema_struct_value.#variant_name.schema, schema_struct_value.#variant_name.hash)
        }
    });

    // Generate serialization arms using the static hashes
    let serialize_arms = variant_names.iter().map(|variant_name| {
        quote! {
            #enum_name::#variant_name(payload) => {
                let hash = schema_struct_value.#variant_name.hash;

                let mut tup = serializer.serialize_tuple(2)?;
                tup.serialize_element(&hash)?;
                tup.serialize_element(payload)?;
                tup.end()
            }
        }
    });

    // Generate deserialization branches using the static hashes
    let deserialize_branches =
        variant_names
            .iter()
            .zip(field_types.iter())
            .map(|(variant_name, field_type)| {
                quote! {
                    if &hash_bytes == &schema_struct_value.#variant_name.hash {
                        let payload = seq.next_element::<#field_type>()?.ok_or_else(||
                            serde::de::Error::custom("missing payload"))?;
                        return Ok(#enum_name::#variant_name(payload));
                    }
                }
            });

    // Generate the implementation
    let generated_impls = quote! {
        // The original enum definition
        #original_enum

        // Define a struct to hold the schema hashes
        #[allow(non_snake_case)]
        struct #schema_struct_name {
            #(#schema_struct_fields),*
        }

        // Create a static instance of our hashes using std::sync::OnceLock
        use std::sync::OnceLock;
        #[allow(non_upper_case_globals)]
        static #schema_struct_static_name: OnceLock<#schema_struct_name> = OnceLock::new();

        impl #schema_struct_name {
            // Create a new instance with all the hashes computed
            fn new() -> Self {
                Self {
                    #(#schema_struct_inits),*
                }
            }

            // Static accessor function to get or initialize the global instance
            fn get() -> &'static Self {
                #schema_struct_static_name.get_or_init(|| Self::new())
            }
        }

        impl #enum_name {
            pub fn schemas() -> impl ::std::iter::Iterator<Item = (&'static str, &'static ::n0_schema::Schema, [u8; 32])> {
                let schema_struct_value = #schema_struct_name::get();
                [#(#schema_struct_to_tuples),*].into_iter()
            }
        }

        // Implementation of serde::Serialize for the enum
        impl serde::Serialize for #enum_name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                use serde::ser::SerializeTuple;
                let schema_struct_value = #schema_struct_name::get();

                match self {
                    #(#serialize_arms),*
                }
            }
        }

        // Implementation of serde::Deserialize for the enum with visitor inside
        impl<'de> serde::Deserialize<'de> for #enum_name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                // Define the visitor struct inside the deserialize implementation
                struct Visitor;

                impl<'de> serde::de::Visitor<'de> for Visitor {
                    type Value = #enum_name;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                        formatter.write_str("a tuple with a hash discriminator and payload")
                    }

                    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
                    where
                        A: serde::de::SeqAccess<'de>,
                    {
                        // Deserialize the hash discriminator (first element)
                        let hash_bytes = seq.next_element::<[u8; 32]>()?.ok_or_else(||
                            serde::de::Error::custom("missing hash"))?;

                        // Get the schema hashes
                        let schema_struct_value = #schema_struct_name::get();

                        // Check against our static hashes
                        #(#deserialize_branches)*

                        // If none matched, return an error
                        Err(serde::de::Error::custom("unknown discriminator"))
                    }
                }

                // Use the locally-defined visitor
                deserializer.deserialize_tuple(2, Visitor)
            }
        }
    };

    // Return the generated code
    TokenStream::from(generated_impls)
}
