use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Data, Fields, Lit};

/// Derive macro for the `State` trait.
///
/// Maps a struct's fields to eko-core channels:
/// - No attribute → `LastValue` (last-write-wins)
/// - `#[state(reducer = "path::to::fn")]` → `BinaryOperatorAggregate`
/// - `#[state(reducer = "path::to::fn", default)]` → `BinaryOperatorAggregate` with default init
/// - `#[state(ephemeral)]` → `EphemeralValue` (not persisted)
#[proc_macro_derive(State, attributes(state))]
pub fn derive_state(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match impl_state(&input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

struct FieldInfo {
    ident: syn::Ident,
    ty: syn::Type,
    channel: ChannelKind,
    #[allow(dead_code)]
    has_default: bool,
}

enum ChannelKind {
    LastValue,
    Reducer(syn::Path),
    Ephemeral,
}

fn parse_field_attrs(field: &syn::Field) -> syn::Result<(ChannelKind, bool)> {
    let mut kind = ChannelKind::LastValue;
    let mut has_default = false;

    for attr in &field.attrs {
        if !attr.path().is_ident("state") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("reducer") {
                let value = meta.value()?;
                let lit: Lit = value.parse()?;
                if let Lit::Str(s) = lit {
                    let path: syn::Path = s.parse()?;
                    kind = ChannelKind::Reducer(path);
                } else {
                    return Err(meta.error("reducer expects a string literal path"));
                }
            } else if meta.path.is_ident("default") {
                has_default = true;
            } else if meta.path.is_ident("ephemeral") {
                kind = ChannelKind::Ephemeral;
            } else {
                return Err(meta.error("unknown state attribute"));
            }
            Ok(())
        })?;
    }

    Ok((kind, has_default))
}

fn impl_state(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(f) => &f.named,
            _ => return Err(syn::Error::new_spanned(input, "State can only be derived for structs with named fields")),
        },
        _ => return Err(syn::Error::new_spanned(input, "State can only be derived for structs")),
    };

    let mut field_infos = Vec::new();
    for field in fields {
        let ident = field.ident.clone().unwrap();
        let ty = field.ty.clone();
        let (channel, has_default) = parse_field_attrs(field)?;
        field_infos.push(FieldInfo { ident, ty, channel, has_default });
    }

    let channels_inserts = field_infos.iter().map(|f| {
        let key = f.ident.to_string();
        match &f.channel {
            ChannelKind::LastValue => quote! {
                m.insert(
                    #key.into(),
                    Box::new(::eko_core::channels::LastValue::new()) as Box<dyn ::eko_core::channels::base::AnyChannel>,
                );
            },
            ChannelKind::Reducer(path) => quote! {
                m.insert(
                    #key.into(),
                    Box::new(::eko_core::channels::BinaryOperatorAggregate::new(#path)) as Box<dyn ::eko_core::channels::base::AnyChannel>,
                );
            },
            ChannelKind::Ephemeral => quote! {
                m.insert(
                    #key.into(),
                    Box::new(::eko_core::channels::EphemeralValue::new()) as Box<dyn ::eko_core::channels::base::AnyChannel>,
                );
            },
        }
    });

    let from_fields = field_infos.iter().map(|f| {
        let ident = &f.ident;
        let key = f.ident.to_string();
        let ty = &f.ty;

        // For Option<T> types, missing key → None; for others, try Default
        if is_option_type(ty) {
            quote! {
                #ident: values.get(#key)
                    .map(|v| ::serde_json::from_value::<#ty>(v.clone()))
                    .transpose()
                    .map_err(|e| ::eko_core::AgentError::Other(e.into()))?
                    .flatten()
            }
        } else {
            quote! {
                #ident: values.get(#key)
                    .map(|v| ::serde_json::from_value::<#ty>(v.clone()))
                    .transpose()
                    .map_err(|e| ::eko_core::AgentError::Other(e.into()))?
                    .unwrap_or_default()
            }
        }
    });

    let to_fields = field_infos.iter().map(|f| {
        let ident = &f.ident;
        let key = f.ident.to_string();
        quote! {
            if let Ok(v) = ::serde_json::to_value(&self.#ident) {
                m.insert(#key.into(), v);
            }
        }
    });

    let key_strs = field_infos.iter().map(|f| {
        let key = f.ident.to_string();
        quote! { #key }
    });

    Ok(quote! {
        impl #impl_generics ::eko_core::State for #name #ty_generics #where_clause {
            fn channels() -> ::std::collections::HashMap<String, Box<dyn ::eko_core::channels::base::AnyChannel>> {
                let mut m = ::std::collections::HashMap::new();
                #(#channels_inserts)*
                m
            }

            fn from_channel_values(
                values: &::eko_core::channels::ChannelValues,
            ) -> Result<Self, ::eko_core::AgentError> {
                Ok(Self {
                    #(#from_fields,)*
                })
            }

            fn to_channel_values(&self) -> ::eko_core::channels::ChannelValues {
                let mut m = ::eko_core::channels::ChannelValues::new();
                #(#to_fields)*
                m
            }

            fn keys() -> Vec<&'static str> {
                vec![#(#key_strs,)*]
            }
        }
    })
}

fn is_option_type(ty: &syn::Type) -> bool {
    if let syn::Type::Path(tp) = ty {
        if let Some(seg) = tp.path.segments.last() {
            return seg.ident == "Option";
        }
    }
    false
}
