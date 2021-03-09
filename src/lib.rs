extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields, Ident, Lit, LitStr, Meta};

#[proc_macro_derive(Resources, attributes(url))]
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

    for field in fields {
        let field_ident = field.ident.expect("tuple struct");
        let url: LitStr = field
            .attrs
            .iter()
            // assume that all attrs are `url` for now
            .map(|attr| attr.parse_meta().expect("expected a string literal"))
            .map(|meta| match meta {
                Meta::Path(_) | Meta::List(_) => panic!("expected name-value attribute"),
                Meta::NameValue(nv) => nv,
            })
            .map(|nv| match nv.lit {
                Lit::Str(s) => s,
                _ => panic!("expected a string literal"),
            })
            .next()
            .expect(r#"missing "url" attribute"#);

        acquire_now = quote! {
            #acquire_now
            #field_ident: resources.acquire_now(#url, MustBeFresh)?,
        };

        acquire_assign = quote! {
            #acquire_assign
            let #field_ident = resources.acquire(#url, MustBeFresh, orders);
        };

        acquire_return = quote! {
            #acquire_return
            #field_ident: #field_ident?,
        };

        let url_get_ident = Ident::new(&format!("{}_url", field_ident), field_ident.span());
        url_getters = quote! {
            #url_getters
            pub fn #url_get_ident() -> &'static str {
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
                resources: &'a ResourceStore,
                orders: &mut impl Orders<M>
            ) -> Result<Self, NotAvailable> {
                #acquire_assign

                Ok(Self {
                    #acquire_return
                })
            }

            pub fn acquire_now(resources: &'a ResourceStore) -> Result<Self, NotAvailable> {
                Ok(Self {
                    #acquire_now
                })
            }

            pub fn has_resource(url: &'static str) -> bool {
                const URLS: &[&'static str] = &[ #url_array ];
                URLS.contains(&url)
            }

            #url_getters
        }
    };

    output.into()
}
