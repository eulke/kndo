//! `#[derive(ContractFingerprint)]` — emits a shape fold built purely from the tokens
//! this macro can see: the type name (stringify), the defining module (module_path!(),
//! which expands at the definition site to source text), field/variant names in
//! declaration order, and a trait-recursive `child::<FieldType>()` per field. The
//! macro never inspects other types — recursion happens through the trait at use time.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields};

#[proc_macro_derive(ContractFingerprint)]
pub fn derive_contract_fingerprint(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let name_str = name.to_string();

    // Measured on rustc 1.94.1 (experiment e11): a `#[cfg]` on a field that SURVIVES
    // cfg-evaluation is still visible in the tokens this derive receives, so refusing
    // it here enforces "contract fields are unconditional" at compile time on every
    // platform where the field exists. The residue: a field whose cfg is false on the
    // building platform is stripped before the derive sees anything — that case is
    // covered by the cross-target fingerprint comparison in CI, not by this guard.
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
                .into()
        }
    };

    // Add `T: ContractFingerprint` for every type parameter so instantiations fold
    // the argument's shape through the same trait.
    let mut generics = input.generics.clone();
    for param in generics.type_params_mut() {
        param.bounds.push(syn::parse_quote!(::fingerprint::ContractFingerprint));
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    quote! {
        #cfg_check
        impl #impl_generics ::fingerprint::ContractFingerprint for #name #ty_generics #where_clause {
            const TAG: &'static str = #name_str;
            const CYCLIC_GUARD: bool = true;
            fn fold(f: &mut ::fingerprint::Fold) {
                // Source text of the defining module — stable, unlike type_name.
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
