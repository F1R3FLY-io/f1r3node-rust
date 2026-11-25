use std::path::Path;
use std::{env, fs};

use proc_macro2::TokenStream;
use quote::quote;

fn visitors(items: &[syn::Item], module: TokenStream, out: &mut TokenStream) {
    for item in items {
        let (name, body, is_par) = match item {
            syn::Item::Struct(s) => {
                let calls = s.fields.iter().enumerate().map(|(i, f)| {
                    let member = f
                        .ident
                        .clone()
                        .map(syn::Member::Named)
                        .unwrap_or_else(|| syn::Member::Unnamed(i.into()));
                    quote! { self.#member.walk(ctx, depth + 1, _child_par_depth)?; }
                });
                (&s.ident, quote! { #(#calls)* }, s.ident == "Par")
            }
            syn::Item::Enum(e) => {
                let arms = e.variants.iter().map(|v| {
                    let name = &v.ident;
                    match &v.fields {
                        syn::Fields::Unnamed(fields) if fields.unnamed.len() == 1 => quote! { Self::#name(value) => value.walk(ctx, depth + 1, _child_par_depth)?, },
                        syn::Fields::Unit => quote! { Self::#name => {}, },
                        _ => panic!("unsupported generated enum layout"),
                    }
                });
                (&e.ident, quote! { match self { #(#arms)* } }, false)
            }
            syn::Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    let name = &m.ident;
                    visitors(items, quote! { #module::#name }, out);
                }
                continue;
            }
            _ => continue,
        };
        let record = if is_par {
            quote! { ctx.record_par(self, par_depth)?; }
        } else {
            quote! {}
        };
        out.extend(quote! {
            impl Walk for #module::#name {
                const HAS_PAR: bool = true;
                fn walk(&self, ctx: &mut WalkContext, depth: usize, par_depth: usize) -> Result<(), Box<dyn std::error::Error>> {
                    ctx.visit(depth)?;
                    let _child_par_depth = par_depth + usize::from(#is_par);
                    #body
                    #record
                    Ok(())
                }
            }
        });
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let out = env::var_os("OUT_DIR").unwrap();
    let out = Path::new(&out);
    let proto = root.join("models/src/main/protobuf/RhoTypes.proto");
    println!("cargo:rerun-if-env-changed=PROTOC");
    println!("cargo:rerun-if-changed={}", proto.display());
    println!(
        "cargo:rerun-if-changed={}",
        root.join("models/build.rs").display()
    );
    let mut config = prost_build::Config::new();
    config.btree_map(["."]);
    config.type_attribute(".rhoapi", "#[derive(serde::Serialize, serde::Deserialize)]");
    config
        .compile_protos(&[proto], &[
            root.join("models/src/main/protobuf"),
            root.join("models/src"),
        ])
        .unwrap();
    let parsed = syn::parse_file(&fs::read_to_string(out.join("rhoapi.rs")).unwrap()).unwrap();
    let mut walks = TokenStream::new();
    visitors(&parsed.items, quote! { rhoapi }, &mut walks);
    fs::write(out.join("leaf_walks.rs"), walks.to_string()).unwrap();
    let mut records = TokenStream::new();
    for (path, names) in [
        (
            "rspace++/src/rspace/internal.rs",
            &["Datum", "WaitingContinuation"][..],
        ),
        (
            "rspace++/src/rspace/trace/event.rs",
            &["Produce", "Consume"][..],
        ),
        (
            "rspace++/src/rspace/history/cold_store.rs",
            &[
                "PersistedData",
                "DataLeaf",
                "ContinuationsLeaf",
                "JoinsLeaf",
            ][..],
        ),
    ] {
        let path = root.join(path);
        println!("cargo:rerun-if-changed={}", path.display());
        let file = syn::parse_file(&fs::read_to_string(path).unwrap()).unwrap();
        let mut found = 0;
        for item in file.items {
            match item {
                syn::Item::Struct(mut s) if names.contains(&s.ident.to_string().as_str()) => {
                    s.attrs = vec![
                        syn::parse_quote!(#[derive(Clone, serde::Serialize, serde::Deserialize)]),
                    ];
                    for f in &mut s.fields {
                        f.attrs.retain(|a| a.path().is_ident("serde"));
                    }
                    records.extend(quote! { #s });
                    found += 1;
                }
                syn::Item::Enum(mut e) if names.contains(&e.ident.to_string().as_str()) => {
                    e.attrs = vec![
                        syn::parse_quote!(#[derive(Clone, serde::Serialize, serde::Deserialize)]),
                    ];
                    records.extend(quote! { #e });
                    found += 1;
                }
                _ => {}
            }
        }
        assert_eq!(found, names.len(), "source record missing");
    }
    fs::write(out.join("leaf_records.rs"), records.to_string()).unwrap();
}
