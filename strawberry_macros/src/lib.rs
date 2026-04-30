use proc_macro::TokenStream;
use quote::__private::Span;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{
    GenericArgument, Ident, ItemStruct, LitStr, PathArguments, Token, Type, TypePath,
    parse_macro_input,
};

fn strip_single_wrapper<'a>(ty: &'a Type, wrapper: &str) -> Option<&'a Type> {
    if let Type::Path(TypePath { qself: None, path }) = ty {
        if let Some(last_segment) = path.segments.last() {
            if last_segment.ident == wrapper {
                if let PathArguments::AngleBracketed(ref args) = last_segment.arguments {
                    if args.args.len() == 1 {
                        if let GenericArgument::Type(inner_ty) = &args.args[0] {
                            return Some(inner_ty);
                        }
                    }
                }
            }
        }
    }
    None
}

fn is_wrapper(ty: &Type, wrapper: &str) -> bool {
    strip_single_wrapper(ty, wrapper).is_some()
}

struct StringArgs {
    strings: Punctuated<LitStr, Token![,]>,
}

impl Parse for StringArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let strings = Punctuated::parse_terminated(input)?;
        Ok(StringArgs { strings })
    }
}

#[proc_macro_attribute]
pub fn builder(_: TokenStream, item: TokenStream) -> TokenStream {
    let a_struct = parse_macro_input!(item as ItemStruct);
    let struct_ident = &a_struct.ident;

    let mut required_constructor_tokens = Vec::new();
    let mut constructor_tokens = Vec::new();
    let mut type_tokens = Vec::new();
    let fields = &a_struct.fields;
    fields.iter().for_each(|field| {
        let ident = &field.ident;
        if ident.is_none() {
            return;
        }
        let ident = ident.as_ref().unwrap();
        let name = quote!(#ident).to_string();

        let mut ty = &field.ty;
        let is_mutex = is_wrapper(ty, "Mutex");
        if is_mutex {
            ty = strip_single_wrapper(ty, "Mutex").unwrap();
        }
        let is_option = is_wrapper(ty, "Option");
        if is_option {
            let inner = strip_single_wrapper(ty, "Option").unwrap();
            let type_ident = Ident::new(quote!(#inner).to_string().as_str(), Span::mixed_site());
            let set_function_ident =
                Ident::new(format!("set_{}", name).as_str(), Span::mixed_site());
            let take_function_ident =
                Ident::new(format!("take_{}", name).as_str(), Span::mixed_site());

            if is_mutex {
                type_tokens.push(quote! {
                    pub fn #set_function_ident(&self, #ident: #type_ident) -> &#struct_ident {
                        let mut lock = self.#ident.lock();
                        *lock = Some(#ident);
                        self
                    }

                    pub fn #take_function_ident(&self) -> Option<#inner> {
                        let mut lock = self.#ident.lock();
                        let data = lock.take();
                        data
                    }
                });
                constructor_tokens.push(quote! {
                    #ident: Mutex::new(None),
                });
            } else {
                type_tokens.push(quote! {
                    pub fn #set_function_ident(&mut self, #ident: #type_ident) -> &#struct_ident {
                        self.#ident = Some(#ident);
                        self
                    }

                    pub fn #take_function_ident(&mut self) -> Option<#inner> {
                        let data = self.#ident.take();
                        data
                    }
                });
                constructor_tokens.push(quote! {
                    #ident: None,
                });
            }
        } else {
            let type_ident = Ident::new(quote!(#ty).to_string().as_str(), Span::mixed_site());
            constructor_tokens.push(quote! {
                #ident: #ident,
            });
            required_constructor_tokens.push(quote! {
                #ident: #type_ident,
            })
        }
    });

    let (impl_generics, ty_generics, where_clause) = a_struct.generics.split_for_impl();
    let expanded = quote! {
        #a_struct

        impl #impl_generics #struct_ident #ty_generics #where_clause {
            pub fn new(#(#required_constructor_tokens)*) -> Self {
                Self {
                    #(#constructor_tokens)*
                }
            }

            #(#type_tokens)*
        }
    };
    eprintln!("{}", expanded);

    expanded.into()
}
