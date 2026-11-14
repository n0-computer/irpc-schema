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
            let parsed = list.parse_nested_meta(|meta| {
                if meta.path.is_ident("name") {
                    let value: syn::LitStr = meta.value()?.parse().map_err(|err| {
                        syn::Error::new(err.span(), "Expected string literal for name parameter")
                    })?;
                    explicit_name = Some(value.value());
                    Ok(())
                } else {
                    Err(meta.error("Unsupported parameter in schema attribute"))
                }
            });
            if let Err(err) = parsed {
                return err.to_compile_error().into();
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

    let mut output = input.clone();
    strip_field_attributes(&mut output.data);
    let expanded = quote! {
        #output

        impl ::n0_schema::HasSchema for #name {
            fn schema() -> ::n0_schema::Schema {
                #schema_impl
            }
        }
    };

    TokenStream::from(expanded)
}

/// Returns the schema of a field.
///
/// `#[schema(atom = "Name")]` on the field makes its schema the atom `Name`,
/// for a field type that does not implement `HasSchema`, such as a type from
/// another crate. Otherwise the schema comes from the type's `HasSchema`.
fn field_schema(field: &syn::Field) -> proc_macro2::TokenStream {
    let mut atom = None;
    for attr in field.attrs.iter().filter(|a| a.path().is_ident("schema")) {
        let parsed = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("atom") {
                let value: syn::LitStr = meta.value()?.parse()?;
                atom = Some(value.value());
                Ok(())
            } else {
                Err(meta.error("expected `atom = \"Name\"`"))
            }
        });
        if let Err(err) = parsed {
            return err.to_compile_error();
        }
    }
    match atom {
        Some(name) => quote! { ::n0_schema::Schema::Atom(#name.to_string()) },
        None => {
            let ty = &field.ty;
            quote! { <#ty as ::n0_schema::HasSchema>::schema() }
        }
    }
}

/// Removes `#[schema(...)]` from all fields, since the compiler does not know it there.
fn strip_field_attributes(data: &mut syn::Data) {
    let strip = |fields: &mut Fields| {
        for field in fields.iter_mut() {
            field.attrs.retain(|a| !a.path().is_ident("schema"));
        }
    };
    match data {
        Data::Struct(data) => strip(&mut data.fields),
        Data::Enum(data) => data.variants.iter_mut().for_each(|v| strip(&mut v.fields)),
        Data::Union(_) => {}
    }
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
                        let field_schema = field_schema(f);
                        quote! {
                            #field_schema
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
                        let field_schema = field_schema(f);
                        quote! {
                            #field_schema
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
                                let field_schema = field_schema(f);
                                quote! {
                                    #field_schema
                                }
                            })
                            .collect(),
                        Fields::Unnamed(fields) => fields
                            .unnamed
                            .iter()
                            .map(|f| {
                                let field_schema = field_schema(f);
                                quote! {
                                    #field_schema
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
                        let field_schema = field_schema(f);
                        quote! {
                            ::n0_schema::Named(#field_name.to_string(), #field_schema)
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
                        let field_schema = field_schema(f);
                        quote! {
                            #field_schema
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
                                    let field_schema = field_schema(f);
                                    let field_name = f.ident.as_ref().unwrap().to_string();
                                    quote! {
                                        ::n0_schema::Named(#field_name.to_string(),#field_schema)
                                    }
                                })
                                .collect::<Vec<_>>();
                            // The fields of a variant are all present, so they are a struct.
                            let schema_type = if named.is_empty() {
                                quote! { ::n0_schema::Schema::Unit }
                            } else {
                                quote! { ::n0_schema::Schema::Struct(vec![#(#named),*]) }
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
                                    let field_schema = field_schema(f);
                                    quote! {
                                        #field_schema
                                    }
                                })
                                .collect::<Vec<_>>();
                            let schema_type = if unnamed.is_empty() {
                                quote! { ::n0_schema::Schema::Unit }
                            } else {
                                quote! { ::n0_schema::Schema::Product(vec![#(#unnamed),*]) }
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

            // An enum with one variant still writes its discriminant, unlike a struct.
            let schema = if variants.is_empty() {
                quote! { ::n0_schema::Schema::Bottom }
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
///
/// The enum is serialized as the schema hash of the case, followed by its
/// payload. It implements `n0_schema::StableEnum`, so it can be part of a
/// [`macro@serialize_union`].
#[proc_macro_attribute]
pub fn serialize_stable(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemEnum);
    stable_enum(input, |ty| {
        quote! { <#ty as ::n0_schema::HasSchema>::schema() }
    })
    .unwrap_or_else(syn::Error::into_compile_error)
    .into()
}

/// This is identical to `serialize_stable`, but for a specific service.
///
/// The schema hashes for each variant will include not just the message type itself,
/// but also the kind (none, oneshot, mpsc) and type of tx and rx channels it uses.
///
/// That way any change to channel kinds and types will result in a different hash,
/// just like a change to the message type itself.
///
/// Usage:
/// ```ignore
/// #[serialize_service(MyService)]
/// enum MyServiceProto {}
/// ```
///
/// This macro requires that `irpc::Channels` is implemented for the given service type
/// for each variant of the enum. It also requires that HasSchema is implemented for
/// all channel payload types.
#[proc_macro_attribute]
pub fn serialize_service(attr: TokenStream, item: TokenStream) -> TokenStream {
    let service = parse_macro_input!(attr as syn::Ident);
    let input = parse_macro_input!(item as ItemEnum);
    stable_enum(input, |ty| {
        quote! { <#ty as ::n0_schema::ChannelsSchema<#service>>::schema() }
    })
    .unwrap_or_else(syn::Error::into_compile_error)
    .into()
}

/// Returns the name and the type of the single unnamed field of each variant.
fn single_field_variants(input: &ItemEnum) -> syn::Result<Vec<(&syn::Ident, &syn::Type)>> {
    input
        .variants
        .iter()
        .map(|variant| match &variant.fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                Ok((&variant.ident, &fields.unnamed[0].ty))
            }
            _ => Err(syn::Error::new_spanned(
                variant,
                "each variant must have a single unnamed field",
            )),
        })
        .collect()
}

/// Generates the hash-discriminated serde impls and `StableEnum` for `input`.
fn stable_enum(
    input: ItemEnum,
    schema_of: impl Fn(&syn::Type) -> proc_macro2::TokenStream,
) -> syn::Result<proc_macro2::TokenStream> {
    let enum_name = &input.ident;
    let enum_str = enum_name.to_string();
    let variants = single_field_variants(&input)?;
    let names: Vec<_> = variants.iter().map(|(name, _)| *name).collect();
    let name_strs: Vec<_> = names.iter().map(|name| name.to_string()).collect();
    let types: Vec<_> = variants.iter().map(|(_, ty)| *ty).collect();
    let schemas: Vec<_> = types.iter().map(|ty| schema_of(ty)).collect();
    let docs: Vec<_> = input.variants.iter().map(|v| doc_text(&v.attrs)).collect();

    // Everything except the impls lives in an anonymous const, so that several
    // of these enums can share a module.
    Ok(quote! {
        #input

        const _: () = {
            #[allow(non_snake_case)]
            struct Schemas {
                #(#names: ::n0_schema::SchemaAndHash),*
            }

            fn schemas() -> &'static Schemas {
                static SCHEMAS: ::std::sync::OnceLock<Schemas> = ::std::sync::OnceLock::new();
                SCHEMAS.get_or_init(|| {
                    let s = Schemas {
                        #(#names: ::n0_schema::SchemaAndHash::from(#schemas)),*
                    };
                    // Two cases with the same schema would share a hash, and the second could never be read.
                    let cases = [#((#name_strs, s.#names.hash)),*];
                    if let Err(err) = ::n0_schema::check_disjoint_cases(#enum_str, cases) {
                        panic!("{err}");
                    }
                    s
                })
            }

            impl #enum_name {
                /// Returns the name, schema and hash of each case.
                pub fn schemas() -> impl ::std::iter::Iterator<Item = (&'static str, &'static ::n0_schema::Schema, [u8; 32])> {
                    let s = schemas();
                    [#((#name_strs, &s.#names.schema, s.#names.hash)),*].into_iter()
                }
            }

            impl ::n0_schema::StableEnum for #enum_name {
                fn case_schemas() -> ::std::vec::Vec<(&'static str, &'static ::n0_schema::Schema, [u8; 32])> {
                    Self::schemas().collect()
                }

                fn case_docs() -> ::std::vec::Vec<&'static str> {
                    ::std::vec![#(#docs),*]
                }

                fn deserialize_payload<'de, A: ::n0_schema::serde::de::SeqAccess<'de>>(
                    hash: &[u8; 32],
                    seq: &mut A,
                ) -> ::std::result::Result<::std::option::Option<Self>, A::Error> {
                    let s = schemas();
                    #(
                        if hash == &s.#names.hash {
                            let payload = seq.next_element::<#types>()?.ok_or_else(||
                                ::n0_schema::serde::de::Error::custom("missing payload"))?;
                            return Ok(Some(#enum_name::#names(payload)));
                        }
                    )*
                    Ok(None)
                }
            }

            impl ::n0_schema::serde::Serialize for #enum_name {
                fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
                where
                    S: ::n0_schema::serde::Serializer,
                {
                    use ::n0_schema::serde::ser::SerializeTuple;
                    let s = schemas();
                    let mut tup = serializer.serialize_tuple(2)?;
                    match self {
                        #(
                            #enum_name::#names(payload) => {
                                tup.serialize_element(&s.#names.hash)?;
                                tup.serialize_element(payload)?;
                            }
                        )*
                    }
                    tup.end()
                }
            }

            impl<'de> ::n0_schema::serde::Deserialize<'de> for #enum_name {
                fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
                where
                    D: ::n0_schema::serde::Deserializer<'de>,
                {
                    deserializer.deserialize_tuple(2, ::n0_schema::__private::StableVisitor::<Self>::new())
                }
            }
        };
    })
}

/// Combines several hash-discriminated enums into one, without changing their wire format.
///
/// Each variant must have a single unnamed field whose type implements
/// `n0_schema::StableEnum`, such as an enum with [`macro@serialize_stable`],
/// [`macro@serialize_service`] or another union.
///
/// A value serializes exactly like the inner value. Deserializing reads the
/// schema hash and picks the variant whose type has a case with this hash.
/// Since the hash identifies the case, not its position, there is no ambiguity,
/// unless two inner enums have a case with the same schema. The first use
/// of the union checks for that, and panics if it happens.
///
/// ```ignore
/// #[serialize_union]
/// #[derive(Debug)]
/// enum Combined {
///     Store(StoreProtocol),
///     Admin(AdminProtocol),
/// }
/// ```
#[proc_macro_attribute]
pub fn serialize_union(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemEnum);
    union_enum(&input)
        .map(|tokens| quote! { #input #tokens })
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Generates the transparent serde impls and `StableEnum` for a union.
fn union_enum(input: &ItemEnum) -> syn::Result<proc_macro2::TokenStream> {
    let enum_name = &input.ident;
    let enum_str = enum_name.to_string();
    let variants = single_field_variants(input)?;
    let names: Vec<_> = variants.iter().map(|(name, _)| *name).collect();
    let types: Vec<_> = variants.iter().map(|(_, ty)| *ty).collect();

    Ok(quote! {
        const _: () = {
            fn check() {
                static CHECKED: ::std::sync::OnceLock<()> = ::std::sync::OnceLock::new();
                CHECKED.get_or_init(|| {
                    let cases = ::std::iter::empty()
                        #(.chain(<#types as ::n0_schema::StableEnum>::case_schemas()))*
                        .map(|(name, _, hash)| (name, hash));
                    if let Err(err) = ::n0_schema::check_disjoint_cases(#enum_str, cases) {
                        panic!("{err}");
                    }
                });
            }

            impl #enum_name {
                /// Returns the name, schema and hash of each case of all inner enums.
                pub fn schemas() -> impl ::std::iter::Iterator<Item = (&'static str, &'static ::n0_schema::Schema, [u8; 32])> {
                    <Self as ::n0_schema::StableEnum>::case_schemas().into_iter()
                }
            }

            impl ::n0_schema::StableEnum for #enum_name {
                fn case_schemas() -> ::std::vec::Vec<(&'static str, &'static ::n0_schema::Schema, [u8; 32])> {
                    check();
                    let mut cases = ::std::vec::Vec::new();
                    #(cases.extend(<#types as ::n0_schema::StableEnum>::case_schemas());)*
                    cases
                }

                fn case_docs() -> ::std::vec::Vec<&'static str> {
                    let mut docs = ::std::vec::Vec::new();
                    #(docs.extend(<#types as ::n0_schema::StableEnum>::case_docs());)*
                    docs
                }

                fn deserialize_payload<'de, A: ::n0_schema::serde::de::SeqAccess<'de>>(
                    hash: &[u8; 32],
                    seq: &mut A,
                ) -> ::std::result::Result<::std::option::Option<Self>, A::Error> {
                    check();
                    #(
                        if let Some(inner) = <#types as ::n0_schema::StableEnum>::deserialize_payload(hash, seq)? {
                            return Ok(Some(#enum_name::#names(inner)));
                        }
                    )*
                    Ok(None)
                }
            }

            impl ::n0_schema::serde::Serialize for #enum_name {
                fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
                where
                    S: ::n0_schema::serde::Serializer,
                {
                    check();
                    match self {
                        #(#enum_name::#names(inner) => ::n0_schema::serde::Serialize::serialize(inner, serializer)),*
                    }
                }
            }

            impl<'de> ::n0_schema::serde::Deserialize<'de> for #enum_name {
                fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
                where
                    D: ::n0_schema::serde::Deserializer<'de>,
                {
                    check();
                    deserializer.deserialize_tuple(2, ::n0_schema::__private::StableVisitor::<Self>::new())
                }
            }
        };
    })
}

/// Combines several irpc services into one, for serving them on one connection.
///
/// This is [`macro@serialize_union`] for enums with [`macro@serialize_service`],
/// plus an `irpc::Service` impl for the union. Clients keep using the inner
/// services, each with its own `Client`, since the union has the same wire
/// format as each of them.
///
/// It generates:
/// - a message enum, named `<Union>Message` or set with `message = Name`, with
///   the message type of each inner service, and `irpc::Service` and
///   `irpc::rpc::RemoteService` for the union.
/// - `handler`, which takes one `irpc::LocalSender` per inner service and
///   returns an `irpc::rpc::Handler` that sends each request to the sender of
///   its service.
///
/// All inner services must agree on span propagation, which is checked at compile time.
/// The generated code needs the `rpc` feature of irpc.
///
/// ```ignore
/// #[combine_services]
/// #[derive(Debug)]
/// enum Combined {
///     Store(StoreProtocol),
///     Admin(AdminProtocol),
/// }
///
/// let handler = Combined::handler(store_tx, admin_tx);
/// ```
#[proc_macro_attribute]
pub fn combine_services(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemEnum);
    let mut message = None;
    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("message") {
            message = Some(meta.value()?.parse::<syn::Ident>()?);
            Ok(())
        } else {
            Err(meta.error("unsupported combine_services argument"))
        }
    });
    parse_macro_input!(attr with parser);
    combine_services_impl(&input, message)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn combine_services_impl(
    input: &ItemEnum,
    message: Option<syn::Ident>,
) -> syn::Result<proc_macro2::TokenStream> {
    let enum_name = &input.ident;
    let vis = &input.vis;
    let union = union_enum(input)?;
    let variants = single_field_variants(input)?;
    let Some((_, first)) = variants.first() else {
        return Err(syn::Error::new_spanned(
            input,
            "combine_services needs at least one service",
        ));
    };
    let names: Vec<_> = variants.iter().map(|(name, _)| *name).collect();
    let types: Vec<_> = variants.iter().map(|(_, ty)| *ty).collect();
    let params: Vec<_> = names
        .iter()
        .map(|name| syn::Ident::new(&snake_case(&name.to_string()), name.span()))
        .collect();
    let message = message
        .unwrap_or_else(|| syn::Ident::new(&format!("{enum_name}Message"), enum_name.span()));
    let irpc = quote! { ::n0_schema::irpc };

    Ok(quote! {
        #input
        #union

        /// The message of one of the combined services.
        #vis enum #message {
            #(#names(<#types as #irpc::Service>::Message)),*
        }

        #(
            const _: () = assert!(
                <#types as #irpc::Service>::SPAN_PROPAGATION == <#first as #irpc::Service>::SPAN_PROPAGATION,
                "combined services must agree on span propagation",
            );
        )*

        impl #irpc::Service for #enum_name {
            type Message = #message;
            const SPAN_PROPAGATION: bool = <#first as #irpc::Service>::SPAN_PROPAGATION;

            fn new_request_span(&self, req_id: u64) -> #irpc::__macro_exports::tracing::Span {
                match self {
                    #(#enum_name::#names(inner) => #irpc::Service::new_request_span(inner, req_id)),*
                }
            }
        }

        impl #irpc::rpc::RemoteService for #enum_name {
            fn with_remote_channels(
                self,
                rx: #irpc::__macro_exports::RecvStream,
                tx: #irpc::__macro_exports::SendStream,
            ) -> Self::Message {
                match self {
                    #(#enum_name::#names(inner) => #message::#names(
                        #irpc::rpc::RemoteService::with_remote_channels(inner, rx, tx)
                    )),*
                }
            }
        }

        impl #enum_name {
            /// Returns a handler that sends each request to the sender of its service.
            #[allow(clippy::too_many_arguments)]
            #vis fn handler(#(#params: impl ::std::convert::Into<#irpc::LocalSender<#types>>),*) -> #irpc::rpc::Handler<Self> {
                #(let #params: #irpc::LocalSender<#types> = #params.into();)*
                #irpc::rpc::Handler::raw(move |msg: Self, rx, tx| {
                    // Attach the channels here, inside the request span.
                    let send: ::std::pin::Pin<::std::boxed::Box<
                        dyn ::std::future::Future<Output = ::std::result::Result<(), #irpc::channel::SendError>> + Send
                    >> = match msg {
                        #(#enum_name::#names(inner) => ::std::boxed::Box::pin(#params.send_raw(
                            #irpc::rpc::RemoteService::with_remote_channels(inner, rx, tx)
                        ))),*
                    };
                    async move {
                        // A stopped service closes the connection, as with `Handler::from_sender`.
                        send.await.map_err(|_| #irpc::rpc::HandlerError::close_connection(
                            #irpc::rpc::VarInt::from_u32(0),
                            b"",
                        ))
                    }
                })
            }
        }
    })
}

/// Converts a variant name like `BlobStore` to a parameter name like `blob_store`.
fn snake_case(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Returns the text of the doc comments in `attrs`, one line per `///` line.
fn doc_text(attrs: &[syn::Attribute]) -> String {
    let lines: Vec<String> = attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .filter_map(|attr| match &attr.meta {
            Meta::NameValue(nv) => match &nv.value {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(text),
                    ..
                }) => Some(text.value()),
                _ => None,
            },
            _ => None,
        })
        .map(|line| line.strip_prefix(' ').unwrap_or(&line).to_owned())
        .collect();
    lines.join("\n")
}
