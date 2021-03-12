extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{
    parse_macro_input, Data, DeriveInput, Fields, Ident, Lit, LitStr, Meta, MetaNameValue, Type,
    TypePath,
};

#[proc_macro_derive(Resources, attributes(url, policy))]
pub fn derive_resources(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let ident = input.ident;
    let generics = input.generics;

    let data = match input.data {
        Data::Struct(data) => data,
        Data::Enum(_) => panic!("expected struct, found enum"),
        Data::Union(_) => panic!("expected struct, found union"),
    };

    let fields = match data.fields {
        Fields::Named(fields) => fields.named.into_iter().collect(),
        Fields::Unit => vec![],
        Fields::Unnamed(_) => panic!("unnamed fields are not supported"),
    };

    let mut acquire_assign = quote! {};
    let mut acquire_return = quote! {};
    let mut acquire_now = quote! {};
    let mut url_getters = quote! {};
    let mut url_array = quote! {};

    let dont_fetch_type: Type = TypePath {
        qself: None,
        path: Ident::new("DontFetch", Span::call_site()).into(),
    }
    .into();

    for field in fields {
        let field_ident = field.ident.expect("tuple struct");
        let nvs: Vec<MetaNameValue> = field
            .attrs
            .iter()
            .map(|attr| attr.parse_meta().expect("expected a string literal"))
            .filter_map(|meta| match meta {
                Meta::Path(_) | Meta::List(_) => None,
                Meta::NameValue(nv) => Some(nv),
            })
            .collect();

        let url: LitStr = nvs
            .iter()
            .filter(|nv| match nv.path.get_ident() {
                Some(ident) => ident == "url",
                _ => false,
            })
            .map(|nv| match &nv.lit {
                Lit::Str(s) => s.clone(),
                _ => panic!("expected a string literal"),
            })
            .next()
            .expect(r#"missing "url" attribute"#);

        let policy = nvs
            .iter()
            .filter(|nv| match nv.path.get_ident() {
                Some(ident) => ident == "policy",
                _ => false,
            })
            .map(|nv| match &nv.lit {
                Lit::Str(s) => {
                    let s = s.value();
                    match s.as_str() {
                        "MustBeFresh" => quote! { ::seed_fetcher::MustBeFresh },
                        "MayBeStale" => quote! { ::seed_fetcher::MustBeFresh },
                        "SilentRefetch" => quote! { ::seed_fetcher::SilentRefetch },
                        _ => panic!("invalid cache-policy: {}", s),
                    }
                }
                _ => panic!("invalid cache-policy, expected a string"),
            })
            .next()
            .unwrap_or(quote! { ::seed_fetcher::MustBeFresh });

        if field.ty == dont_fetch_type {
            acquire_now = quote! {
                #acquire_now
                #field_ident: #dont_fetch_type,
            };

            acquire_return = quote! {
                #acquire_return
                #field_ident: #dont_fetch_type,
            };
        } else {
            acquire_now = quote! {
                #acquire_now
                #field_ident: resources.acquire_now(#url, #policy)?,
            };

            acquire_assign = quote! {
                #acquire_assign
                let #field_ident = resources.acquire(#url, #policy, orders);
            };

            acquire_return = quote! {
                #acquire_return
                #field_ident: #field_ident?,
            };
        }

        let url_get_ident = Ident::new(&format!("{}_url", field_ident), field_ident.span());
        url_getters = quote! {
            #url_getters
            pub fn #url_get_ident() -> ::seed_fetcher::Resource {
                #url
            }
        };

        url_array = quote! {
            #url_array
            #url,
        };
    }

    let output = quote! {
        impl#generics #ident #generics {
            pub fn acquire<M: 'static>(
                resources: &'a ::seed_fetcher::ResourceStore,
                orders: &mut impl ::seed::app::orders::Orders<M>
            ) -> Result<Self, ::seed_fetcher::NotAvailable> {
                #acquire_assign

                Ok(Self {
                    #acquire_return
                })
            }

            pub fn acquire_now(resources: &'a ::seed_fetcher::ResourceStore) -> Result<Self, ::seed_fetcher::NotAvailable> {
                Ok(Self {
                    #acquire_now
                })
            }

            pub fn has_resource(r: ::seed_fetcher::Resource) -> bool {
                const RES: &[&'static str] = &[ #url_array ];
                RES.contains(&r)
            }

            #url_getters
        }
    };

    output.into()
}
