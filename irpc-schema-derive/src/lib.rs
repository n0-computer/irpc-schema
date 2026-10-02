extern crate proc_macro;

use std::collections::BTreeMap;

use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::ParseStream, parse_macro_input, Data, DeriveInput, Fields, ItemEnum, Meta, Token,
};

// The attribute macro for schema generation
#[proc_macro_attribute]
pub fn schema(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    // The field attributes are read below, and must not stay on the output.
    let mut output = input.clone();
    strip_field_attrs(&mut output);
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

    let schema_impl = impl_has_schema(&input, schema_impl);
    let expanded = quote! {
        #output

        #schema_impl
    };

    TokenStream::from(expanded)
}

/// Returns the `HasSchema` impl for `input`, with a `HasSchema` bound on each type parameter.
fn impl_has_schema(
    input: &DeriveInput,
    schema_impl: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let name = &input.ident;
    let mut generics = input.generics.clone();
    for param in generics.type_params_mut() {
        param
            .bounds
            .push(syn::parse_quote!(::irpc_schema::HasSchema));
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    quote! {
        impl #impl_generics ::irpc_schema::HasSchema for #name #ty_generics #where_clause {
            fn schema() -> ::irpc_schema::Schema {
                #schema_impl
            }
        }
    }
}

/// Derives `HasSchema` with a nominal schema named after the type.
///
/// This is the same as `#[schema(Nominal)]`, but as a derive. Use it where
/// only derives are allowed, for example in `#[wrap(Name, derive(HasSchema))]`
/// of the irpc `rpc_requests` macro.
///
/// A field of a type without a schema, for example from another crate, can be
/// named as an atom with `#[schema(atom = "other_crate::Type")]`.
#[proc_macro_derive(HasSchema, attributes(schema))]
pub fn derive_has_schema(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let schema_impl = generate_nominal_schema(&input.ident, &input.data, None);
    impl_has_schema(&input, schema_impl).into()
}

/// Returns the schema of a field: the schema of its type, or the atom set by `#[schema(atom = "..")]`.
fn field_schema(field: &syn::Field) -> proc_macro2::TokenStream {
    match field_atom(field) {
        Some(name) => quote! { ::irpc_schema::Schema::Atom(#name.to_string()) },
        None => {
            let ty = &field.ty;
            quote! { <#ty as ::irpc_schema::HasSchema>::schema() }
        }
    }
}

/// Returns the name in a `#[schema(atom = "..")]` attribute on a field.
///
/// # Panics
///
/// Panics on a `schema` attribute of another form, like the rest of this macro.
fn field_atom(field: &syn::Field) -> Option<String> {
    let attr = field
        .attrs
        .iter()
        .find(|attr| attr.path.is_ident("schema"))?;
    if let Ok(Meta::List(list)) = attr.parse_meta() {
        if let [syn::NestedMeta::Meta(Meta::NameValue(nv))] =
            &list.nested.iter().collect::<Vec<_>>()[..]
        {
            if let (true, syn::Lit::Str(name)) = (nv.path.is_ident("atom"), &nv.lit) {
                return Some(name.value());
            }
        }
    }
    panic!("expected `#[schema(atom = \"..\")]` on a field");
}

/// Removes the `schema` attributes from the fields of `input`.
fn strip_field_attrs(input: &mut DeriveInput) {
    let fields: Vec<&mut syn::Field> = match &mut input.data {
        Data::Struct(data) => data.fields.iter_mut().collect(),
        Data::Enum(data) => data
            .variants
            .iter_mut()
            .flat_map(|variant| variant.fields.iter_mut())
            .collect(),
        Data::Union(data) => data.fields.named.iter_mut().collect(),
    };
    for field in fields {
        field.attrs.retain(|attr| !attr.path.is_ident("schema"));
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
        ::irpc_schema::Schema::Atom(#type_name.to_string())
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
                        let schema = field_schema(f);
                        quote! {
                            #schema
                        }
                    })
                    .collect();
                if types.is_empty() {
                    quote! {
                        ::irpc_schema::Schema::Unit
                    }
                } else {
                    quote! {
                        ::irpc_schema::Schema::Product(vec![#(#types),*])
                    }
                }
            }
            Fields::Unnamed(fields) => {
                let types: Vec<proc_macro2::TokenStream> = fields
                    .unnamed
                    .iter()
                    .map(|f| {
                        let schema = field_schema(f);
                        quote! {
                            #schema
                        }
                    })
                    .collect();
                if types.is_empty() {
                    quote! {
                        ::irpc_schema::Schema::Unit
                    }
                } else {
                    quote! {
                        ::irpc_schema::Schema::Product(vec![#(#types),*])
                    }
                }
            }
            Fields::Unit => quote! {
                ::irpc_schema::Schema::Unit
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
                                let schema = field_schema(f);
                                quote! {
                                    #schema
                                }
                            })
                            .collect(),
                        Fields::Unnamed(fields) => fields
                            .unnamed
                            .iter()
                            .map(|f| {
                                let schema = field_schema(f);
                                quote! {
                                    #schema
                                }
                            })
                            .collect(),
                        Fields::Unit => vec![],
                    };
                    if variant_fields.is_empty() {
                        quote! {
                            ::irpc_schema::Schema::Unit
                        }
                    } else {
                        quote! {
                            ::irpc_schema::Schema::Product(vec![#(#variant_fields),*])
                        }
                    }
                })
                .collect();
            if variant_schemas.is_empty() {
                return quote! {
                    ::irpc_schema::Schema::Bottom
                };
            }
            quote! {
                ::irpc_schema::Schema::Sum(vec![#(#variant_schemas),*])
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
                        let schema = field_schema(f);
                        quote! {
                            ::irpc_schema::Named(#field_name.to_string(), #schema)
                        }
                    })
                    .collect();
                let schema = if field_schemas.is_empty() {
                    quote! { ::irpc_schema::Schema::Unit }
                } else {
                    quote! { ::irpc_schema::Schema::Struct(vec![#(#field_schemas),*]) }
                };
                quote! {
                    ::irpc_schema::Schema::Named(
                        Box::new(::irpc_schema::Named(#name_text.to_string(), #schema))
                    )
                }
            }
            Fields::Unnamed(fields) => {
                let field_schemas: Vec<proc_macro2::TokenStream> = fields
                    .unnamed
                    .iter()
                    .map(|f| {
                        let schema = field_schema(f);
                        quote! {
                            #schema
                        }
                    })
                    .collect();
                let schema = if field_schemas.is_empty() {
                    quote! { ::irpc_schema::Schema::Unit }
                } else {
                    quote! { ::irpc_schema::Schema::Product(vec![#(#field_schemas),*]) }
                };
                quote! {
                    ::irpc_schema::Schema::Named(
                        Box::new(::irpc_schema::Named(#name_text.to_string(), #schema))
                    )
                }
            }
            Fields::Unit => quote! {
                ::irpc_schema::Schema::Named(
                    Box::new(::irpc_schema::Named(#name_text.to_string(), ::irpc_schema::Schema::Unit))
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
                                    let schema = field_schema(f);
                                    let field_name = f.ident.as_ref().unwrap().to_string();
                                    quote! {
                                        ::irpc_schema::Named(#field_name.to_string(),#schema)
                                    }
                                })
                                .collect::<Vec<_>>();
                            let schema_type = if named.is_empty() {
                                quote! { ::irpc_schema::Schema::Unit }
                            } else if named.len() == 1 {
                                quote! { ::irpc_schema::Schema::Struct(vec![#(#named),*]) }
                            } else {
                                quote! { ::irpc_schema::Schema::Enum(vec![#(#named),*]) }
                            };
                            quote! {
                                ::irpc_schema::Named(
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
                                    let schema = field_schema(f);
                                    quote! {
                                        #schema
                                    }
                                })
                                .collect::<Vec<_>>();
                            let schema_type = if unnamed.is_empty() {
                                quote! { ::irpc_schema::Schema::Unit }
                            } else if unnamed.len() == 1 {
                                quote! { ::irpc_schema::Schema::Product(vec![#(#unnamed),*]) }
                            } else {
                                quote! { ::irpc_schema::Schema::Sum(vec![#(#unnamed),*]) }
                            };
                            quote! {
                                ::irpc_schema::Named(
                                    #variant_name_text.to_string(),
                                    #schema_type
                                )
                            }
                        }
                        Fields::Unit => {
                            quote! {
                                ::irpc_schema::Named(
                                    #variant_name_text.to_string(),
                                    ::irpc_schema::Schema::Unit
                                )
                            }
                        }
                    }
                })
                .collect::<Vec<_>>();

            let schema = if variants.is_empty() {
                quote! { ::irpc_schema::Schema::Bottom }
            } else if variants.len() == 1 {
                quote! { ::irpc_schema::Schema::Struct(vec![#(#variants),*]) }
            } else {
                quote! { ::irpc_schema::Schema::Enum(vec![#(#variants),*]) }
            };
            quote! {
                ::irpc_schema::Schema::Named(
                    Box::new(::irpc_schema::Named(#name_text.to_string(), #schema))
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
/// A variant marked `#[flatten]` holds another stable enum. It serializes
/// exactly like the inner enum, and deserializes from any hash that the inner
/// enum knows. If several variants know a hash, the first one gets it.
#[proc_macro_attribute]
pub fn serialize_stable(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemEnum);
    stable_enum(input, None, |field_type| {
        quote! { <#field_type as ::irpc_schema::HasSchema>::schema() }
    })
    .unwrap_or_else(syn::Error::into_compile_error)
    .into()
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
/// ```rust,ignore
/// #[rpc_requests(message = MyServiceMessage)]
/// #[serialize_service]
/// enum MyServiceProto {
///     #[rpc(tx = oneshot::Sender<u64>)]
///     Get(GetRequest),
/// }
/// ```
///
/// This macro requires that `irpc::Channels` is implemented for the given service type
/// for each variant of the enum. It also requires that HasSchema is implemented for
/// all channlels payload types.
///
/// The service type is the enum itself, as with `rpc_requests`. If the `Channels`
/// impls are for another type, name it with `#[serialize_service(channels = MyService)]`.
#[proc_macro_attribute]
pub fn serialize_service(attr: TokenStream, item: TokenStream) -> TokenStream {
    let parser = |input: ParseStream| parse_args(input, &["channels"]);
    let mut args = parse_macro_input!(attr with parser);
    let channels = args.remove("channels");
    let input = parse_macro_input!(item as ItemEnum);
    let channels = channels.unwrap_or_else(|| input.ident.clone());
    stable_enum(input, None, |field_type| {
        quote! { <#field_type as ::irpc_schema::ChannelsSchema<#channels>>::schema() }
    })
    .unwrap_or_else(syn::Error::into_compile_error)
    .into()
}

/// Merges several irpc services into one.
///
/// Each variant holds the protocol enum of a service that uses
/// `serialize_service`. The merged enum accepts the requests of each service, in
/// the wire format of their own clients. If several services know a schema
/// hash, the first variant gets the request.
///
/// The macro implements serde and `StableEnum` for the enum, and generates the
/// message enum given by `message`, with one variant per inner message enum. It
/// implements `irpc::Service` and `irpc::rpc::RemoteService` for the enum.
///
/// With `handler = Name`, the macro also generates a struct with one
/// `irpc::rpc::Handler` per service, in a field named after the variant in snake
/// case. The struct converts into a handler for the merged enum, which passes
/// each request to the handler of its service.
///
/// ```rust,ignore
/// #[merge_services(message = AppMessage, handler = AppHandler)]
/// #[derive(Debug)]
/// enum AppProtocol {
///     Ping(PingProtocol),
///     Store(StoreProtocol),
/// }
///
/// let handler = AppHandler {
///     ping: ping_local_sender.into(),
///     store: store_handler,
/// };
/// let protocol = IrohProtocol::new(handler);
/// ```
#[proc_macro_attribute]
pub fn merge_services(attr: TokenStream, item: TokenStream) -> TokenStream {
    let parser = |input: ParseStream| parse_args(input, &["message", "handler"]);
    let mut args = parse_macro_input!(attr with parser);
    let input = parse_macro_input!(item as ItemEnum);
    let Some(message) = args.remove("message") else {
        return syn::Error::new_spanned(&input.ident, "expected `message = Name`")
            .into_compile_error()
            .into();
    };
    let handler = args
        .remove("handler")
        .map(|handler| handler_impl(&input, &handler));
    let merged = stable_enum(input, Some(message), |_| {
        unreachable!("every variant is flattened")
    })
    .unwrap_or_else(syn::Error::into_compile_error);
    quote! {
        #merged
        #handler
    }
    .into()
}

/// Parses arguments of the form `key = Ident`, where each key is one of `keys`.
fn parse_args(input: ParseStream, keys: &[&str]) -> syn::Result<BTreeMap<String, syn::Ident>> {
    let mut args = BTreeMap::new();
    while !input.is_empty() {
        let key: syn::Ident = input.parse()?;
        if !keys.iter().any(|name| key == name) {
            let expected: Vec<_> = keys.iter().map(|name| format!("`{name}`")).collect();
            let message = format!("expected {}", expected.join(" or "));
            return Err(syn::Error::new(key.span(), message));
        }
        input.parse::<Token![=]>()?;
        args.insert(key.to_string(), input.parse()?);
        if !input.is_empty() {
            input.parse::<Token![,]>()?;
        }
    }
    Ok(args)
}

/// Generates the handler struct of a merged service, and its conversion into a handler.
///
/// Variants with another shape than one unnamed field are skipped here, because
/// `stable_enum` reports them.
fn handler_impl(input: &ItemEnum, handler: &syn::Ident) -> proc_macro2::TokenStream {
    let enum_name = &input.ident;
    let vis = &input.vis;
    let irpc = quote! { ::irpc_schema::__private::irpc };
    let mut variant_names = Vec::new();
    let mut field_names = Vec::new();
    let mut field_types = Vec::new();
    let mut field_docs = Vec::new();
    for variant in &input.variants {
        let Fields::Unnamed(fields) = &variant.fields else {
            continue;
        };
        let [field] = fields.unnamed.iter().collect::<Vec<_>>()[..] else {
            continue;
        };
        let name = snake_case(&variant.ident.to_string());
        let mut field_name = syn::parse_str::<syn::Ident>(&name)
            .unwrap_or_else(|_| syn::Ident::new_raw(&name, variant.ident.span()));
        field_name.set_span(variant.ident.span());
        variant_names.push(&variant.ident);
        field_names.push(field_name);
        field_types.push(&field.ty);
        field_docs.push(format!("Handles the requests of `{}`.", variant.ident));
    }
    let doc = format!("One handler per service of [`{enum_name}`].");
    quote! {
        #[doc = #doc]
        ///
        /// Converts into a handler for the merged enum, which passes each request
        /// to the handler of its service.
        #[derive(Debug, Clone)]
        #vis struct #handler {
            #(
                #[doc = #field_docs]
                pub #field_names: #irpc::rpc::Handler<#field_types>,
            )*
        }

        impl ::std::convert::From<#handler> for #irpc::rpc::Handler<#enum_name> {
            fn from(handler: #handler) -> Self {
                #irpc::rpc::Handler::raw(move |msg, rx, tx| {
                    let handler = handler.clone();
                    async move {
                        match msg {
                            #(
                                #enum_name::#variant_names(msg) => {
                                    handler.#field_names.call(msg, rx, tx).await
                                }
                            )*
                        }
                    }
                })
            }
        }
    }
}

/// Converts a variant name like `KvStore` into a field name like `kv_store`.
fn snake_case(name: &str) -> String {
    let mut out = String::new();
    for (index, c) in name.chars().enumerate() {
        if c.is_uppercase() {
            if index > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Generates the serde impls, the `StableEnum` impl, and the `schemas` function for a stable enum.
///
/// `schema_of` returns an expression that evaluates to the schema of a
/// variant with the given field type. If `merge` is set, every variant is
/// flattened, and the output also contains the message enum with that name
/// and the irpc service impls.
///
/// All helper items live in an anonymous `const` block, so several stable
/// enums can share a module. Serde is reached through a re-export in
/// `irpc_schema`, so the user crate does not need a `serde` dependency
/// under that name.
fn stable_enum(
    mut input: ItemEnum,
    merge: Option<syn::Ident>,
    schema_of: impl Fn(&syn::Type) -> proc_macro2::TokenStream,
) -> syn::Result<proc_macro2::TokenStream> {
    let enum_name = &input.ident;

    // Split the variants into payload variants and flattened variants.
    let mut payload_names = Vec::new();
    let mut payload_types = Vec::new();
    let mut flatten_names = Vec::new();
    let mut flatten_types = Vec::new();
    // Each variant in declaration order, as (name, type, flatten).
    let mut variants = Vec::new();
    for variant in &mut input.variants {
        let attr_count = variant.attrs.len();
        variant.attrs.retain(|attr| !attr.path.is_ident("flatten"));
        let flatten = merge.is_some() || variant.attrs.len() != attr_count;
        let ty = match &variant.fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => fields.unnamed[0].ty.clone(),
            _ => {
                return Err(syn::Error::new_spanned(
                    &variant.fields,
                    "each variant must have a single unnamed field",
                ))
            }
        };
        variants.push((variant.ident.clone(), ty.clone(), flatten));
        if flatten {
            flatten_names.push(variant.ident.clone());
            flatten_types.push(ty);
        } else {
            payload_names.push(variant.ident.clone());
            payload_types.push(ty);
        }
    }

    let schema_struct_inits =
        payload_names
            .iter()
            .zip(&payload_types)
            .map(|(variant_name, field_type)| {
                let schema = schema_of(field_type);
                quote! {
                    #variant_name: ::irpc_schema::SchemaAndHash::from(#schema)
                }
            });
    let payload_name_strs: Vec<_> = payload_names.iter().map(|name| name.to_string()).collect();
    let enum_name_str = enum_name.to_string();

    // A hash can only lead to one variant. So the first variant in
    // declaration order that knows a hash gets it, for listing and for
    // deserializing.
    let push_entries = variants.iter().map(|(name, ty, flatten)| {
        if *flatten {
            quote! {
                for entry in <#ty as ::irpc_schema::StableEnum>::schemas() {
                    push(entry);
                }
            }
        } else {
            let name_str = name.to_string();
            quote! {
                push((#name_str, &schemas.#name.schema, schemas.#name.hash));
            }
        }
    });
    let deserialize_branches = variants.iter().map(|(name, ty, flatten)| {
        if *flatten {
            quote! {
                if let ::std::option::Option::Some(inner) =
                    <#ty as ::irpc_schema::StableEnum>::deserialize_payload(hash, seq)?
                {
                    return ::std::result::Result::Ok(::std::option::Option::Some(Self::#name(inner)));
                }
            }
        } else {
            quote! {
                if *hash == schemas.#name.hash {
                    let payload = seq.next_element::<#ty>()?.ok_or_else(||
                        <A::Error as serde::de::Error>::custom("missing payload"))?;
                    return ::std::result::Result::Ok(::std::option::Option::Some(Self::#name(payload)));
                }
            }
        }
    });

    let service_impls = match merge {
        None => quote! {},
        Some(message) => service_impls(&input, &message, &flatten_names, &flatten_types),
    };

    Ok(quote! {
        #input

        #service_impls

        const _: () = {
            use ::irpc_schema::__private::serde;

            #[allow(non_snake_case)]
            #[derive(Debug)]
            struct Schemas {
                #(#payload_names: ::irpc_schema::SchemaAndHash),*
            }

            static SCHEMAS: ::std::sync::OnceLock<Schemas> = ::std::sync::OnceLock::new();

            impl Schemas {
                /// Returns the schemas of the payload variants.
                ///
                /// # Panics
                ///
                /// Panics on first use if two payload variants have the same
                /// schema hash, because the second one could never be
                /// deserialized.
                fn get() -> &'static Self {
                    SCHEMAS.get_or_init(|| {
                        let schemas = Self {
                            #(#schema_struct_inits),*
                        };
                        let mut seen = ::std::collections::BTreeMap::new();
                        let own: &[(&str, [u8; 32])] = &[#((#payload_name_strs, schemas.#payload_names.hash)),*];
                        for &(name, hash) in own {
                            if let ::std::option::Option::Some(other) = seen.insert(hash, name) {
                                ::std::panic!(
                                    "{} and {} in {} have the same schema hash",
                                    other,
                                    name,
                                    #enum_name_str,
                                );
                            }
                        }
                        schemas
                    })
                }
            }

            type Entry = (&'static str, &'static ::irpc_schema::Schema, [u8; 32]);

            static ENTRIES: ::std::sync::OnceLock<::std::vec::Vec<Entry>> = ::std::sync::OnceLock::new();

            /// Returns one entry per schema hash, in declaration order.
            ///
            /// A hash that a flattened enum shares with an earlier variant
            /// belongs to the earlier variant.
            fn entries() -> &'static [Entry] {
                ENTRIES.get_or_init(|| {
                    let schemas = Schemas::get();
                    let mut entries = ::std::vec::Vec::new();
                    let mut seen = ::std::collections::BTreeSet::new();
                    let mut push = |entry: Entry| {
                        if seen.insert(entry.2) {
                            entries.push(entry);
                        }
                    };
                    #(#push_entries)*
                    let _ = schemas;
                    entries
                })
            }

            impl ::irpc_schema::StableEnum for #enum_name {
                fn schemas() -> impl ::std::iter::Iterator<Item = (&'static str, &'static ::irpc_schema::Schema, [u8; 32])> {
                    entries().iter().copied()
                }

                fn deserialize_payload<'de, A: serde::de::SeqAccess<'de>>(
                    hash: &[u8; 32],
                    seq: &mut A,
                ) -> ::std::result::Result<::std::option::Option<Self>, A::Error> {
                    let schemas = Schemas::get();
                    #(#deserialize_branches)*
                    let _ = (schemas, seq);
                    ::std::result::Result::Ok(::std::option::Option::None)
                }
            }

            impl #enum_name {
                /// Returns the name, schema, and schema hash of each variant.
                pub fn schemas() -> impl ::std::iter::Iterator<Item = (&'static str, &'static ::irpc_schema::Schema, [u8; 32])> {
                    <Self as ::irpc_schema::StableEnum>::schemas()
                }
            }

            impl serde::Serialize for #enum_name {
                fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
                where
                    S: serde::Serializer,
                {
                    use serde::ser::SerializeTuple;
                    match self {
                        #(
                            Self::#payload_names(payload) => {
                                let mut tup = serializer.serialize_tuple(2)?;
                                tup.serialize_element(&Schemas::get().#payload_names.hash)?;
                                tup.serialize_element(payload)?;
                                tup.end()
                            }
                        )*
                        #(
                            Self::#flatten_names(inner) => serde::Serialize::serialize(inner, serializer),
                        )*
                    }
                }
            }

            impl<'de> serde::Deserialize<'de> for #enum_name {
                fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
                where
                    D: serde::Deserializer<'de>,
                {
                    struct Visitor;

                    impl<'de> serde::de::Visitor<'de> for Visitor {
                        type Value = #enum_name;

                        fn expecting(&self, formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                            formatter.write_str("a tuple with a hash discriminator and payload")
                        }

                        fn visit_seq<A>(self, mut seq: A) -> ::std::result::Result<Self::Value, A::Error>
                        where
                            A: serde::de::SeqAccess<'de>,
                        {
                            let hash = seq.next_element::<[u8; 32]>()?.ok_or_else(||
                                serde::de::Error::custom("missing hash"))?;
                            <#enum_name as ::irpc_schema::StableEnum>::deserialize_payload(&hash, &mut seq)?
                                .ok_or_else(|| serde::de::Error::custom("unknown discriminator"))
                        }
                    }

                    deserializer.deserialize_tuple(2, Visitor)
                }
            }
        };
    })
}

/// Generates the message enum and the irpc service impls for a merged service.
fn service_impls(
    input: &ItemEnum,
    message: &syn::Ident,
    flatten_names: &[syn::Ident],
    flatten_types: &[syn::Type],
) -> proc_macro2::TokenStream {
    let enum_name = &input.ident;
    let vis = &input.vis;
    let irpc = quote! { ::irpc_schema::__private::irpc };
    let doc = format!("Message enum for [`{enum_name}`].");
    quote! {
        #[doc = #doc]
        #[derive(Debug)]
        #vis enum #message {
            #(#flatten_names(<#flatten_types as #irpc::Service>::Message)),*
        }

        #(
            impl ::std::convert::From<<#flatten_types as #irpc::Service>::Message> for #message {
                fn from(value: <#flatten_types as #irpc::Service>::Message) -> Self {
                    Self::#flatten_names(value)
                }
            }
        )*

        impl #irpc::Service for #enum_name {
            type Message = #message;
        }

        impl #irpc::rpc::RemoteService for #enum_name {
            fn with_remote_channels(
                self,
                rx: #irpc::__macro_exports::RecvStream,
                tx: #irpc::__macro_exports::SendStream,
            ) -> Self::Message {
                match self {
                    #(
                        Self::#flatten_names(inner) => #message::#flatten_names(
                            <#flatten_types as #irpc::rpc::RemoteService>::with_remote_channels(inner, rx, tx),
                        ),
                    )*
                }
            }
        }
    }
}
