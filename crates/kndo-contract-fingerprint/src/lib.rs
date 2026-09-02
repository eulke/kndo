//! `#[derive(ContractFingerprint)]` — a shape fold built purely from the tokens this
//! macro can see (type name, defining module via `module_path!()`, field/variant names
//! in declaration order) plus trait-recursive folds of field types at use time. The
//! spike verdict (v2/spikes/fingerprint/VERDICT.md) is the spec: no `type_name`, no
//! `TypeId`, cycle-guarded recursion, and a compile error on cfg-gated fields —
//! measured on rustc 1.94.1+: a `#[cfg]` that survives evaluation is still visible
//! here, so the "contract fields are unconditional" policy is compile-time-enforced.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, parse_macro_input};

#[proc_macro_derive(ContractFingerprint)]
pub fn derive_contract_fingerprint(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let name_str = name.to_string();

    let mut cfg_check = quote! {};
    let all_fields: Vec<&syn::Field> = match &input.data {
        Data::Struct(s) => s.fields.iter().collect(),
        Data::Enum(e) => e.variants.iter().flat_map(|v| v.fields.iter()).collect(),
        Data::Union(_) => Vec::new(),
    };
    for field in &all_fields {
        if field.attrs.iter().any(|a| a.path().is_ident("cfg")) {
            cfg_check = quote! {
                compile_error!("ContractFingerprint: cfg-gated fields make the shape platform-dependent; contract types must be unconditional");
            };
        }
    }

    let body = match &input.data {
        Data::Struct(s) => {
            let fields = fold_fields(&s.fields);
            quote! {
                f.atom("struct");
                #fields
            }
        }
        Data::Enum(e) => {
            let variants = e.variants.iter().map(|v| {
                let vname = v.ident.to_string();
                let fields = fold_fields(&v.fields);
                quote! {
                    f.atom("variant");
                    f.atom(#vname);
                    #fields
                }
            });
            quote! {
                f.atom("enum");
                #(#variants)*
            }
        }
        Data::Union(_) => {
            return quote! { compile_error!("ContractFingerprint: unions are not contract types"); }
                .into();
        }
    };

    let mut generics = input.generics.clone();
    for param in generics.type_params_mut() {
        param.bounds.push(syn::parse_quote!(
            ::kndo_contract::fingerprint::ContractFingerprint
        ));
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    quote! {
        #cfg_check
        impl #impl_generics ::kndo_contract::fingerprint::ContractFingerprint for #name #ty_generics #where_clause {
            const TAG: &'static str = #name_str;
            const CYCLIC_GUARD: bool = true;
            fn fold(f: &mut ::kndo_contract::fingerprint::Fold) {
                f.atom(module_path!());
                #body
            }
        }
    }
    .into()
}

fn fold_fields(fields: &Fields) -> proc_macro2::TokenStream {
    match fields {
        Fields::Named(named) => {
            let each = named.named.iter().map(|field| {
                let fname = field.ident.as_ref().unwrap().to_string();
                let ty = &field.ty;
                quote! {
                    f.atom("field");
                    f.atom(#fname);
                    f.child::<#ty>();
                }
            });
            quote! { #(#each)* }
        }
        Fields::Unnamed(unnamed) => {
            let each = unnamed.unnamed.iter().enumerate().map(|(i, field)| {
                let idx = i.to_string();
                let ty = &field.ty;
                quote! {
                    f.atom("field");
                    f.atom(#idx);
                    f.child::<#ty>();
                }
            });
            quote! { #(#each)* }
        }
        Fields::Unit => quote! { f.atom("unit"); },
    }
}
