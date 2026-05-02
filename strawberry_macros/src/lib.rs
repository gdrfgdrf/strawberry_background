use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{GenericArgument, ItemStruct, PathArguments, Type, TypePath, parse_macro_input};

fn strip_single_wrapper<'a>(ty: &'a Type, wrapper: &str) -> Option<&'a Type> {
    if let Type::Path(TypePath { qself: _none, path }) = ty {
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

#[proc_macro_attribute]
pub fn builder(_: TokenStream, item: TokenStream) -> TokenStream {
    let a_struct = parse_macro_input!(item as ItemStruct);
    let struct_ident = &a_struct.ident;

    let mut required_constructor_parameter_tokens = Vec::new();
    let mut constructor_tokens = Vec::new();

    let mut type_tokens = Vec::new();
    let fields = &a_struct.fields;
    fields.iter().for_each(|field| {
        let ident = &field.ident;
        if ident.is_none() {
            return;
        }
        let ident = ident.as_ref().unwrap();

        let mut ty = &field.ty;
        let is_mutex = is_wrapper(ty, "Mutex");
        if is_mutex {
            ty = strip_single_wrapper(ty, "Mutex").unwrap();
        }
        let is_option = is_wrapper(ty, "Option");
        if is_option {
            let ty = strip_single_wrapper(ty, "Option").unwrap();
            let set_function_ident = format_ident!("set_{}", ident);
            let take_function_ident = format_ident!("take_{}", ident);

            let is_vec = is_wrapper(ty, "Vec");
            if is_mutex {
                if is_vec {
                    let ty = strip_single_wrapper(ty, "Vec").unwrap();
                    let push_function_ident = format_ident!("push_{}", ident);
                    let clear_function_ident = format_ident!("clear_{}", ident);

                    type_tokens.push(quote! {
                        pub fn #push_function_ident(&self, item: #ty) -> &#struct_ident {
                            let mut lock = self.#ident.lock();
                            if lock.is_none() {
                                *lock = Some(Vec::new());
                            }
                            let lock = lock.as_mut().unwrap();
                            lock.push(item);
                            self
                        }

                        pub fn #clear_function_ident(&self) -> &#struct_ident {
                            let mut lock = self.#ident.lock();
                            if lock.is_none() {
                                return self;
                            }
                            let lock = lock.as_mut().unwrap();
                            lock.clear();
                            self
                        }
                    });
                }

                type_tokens.push(quote! {
                    pub fn #set_function_ident(&self, #ident: #ty) -> &#struct_ident {
                        let mut lock = self.#ident.lock();
                        *lock = Some(#ident);
                        self
                    }

                    pub fn #take_function_ident(&self) -> Option<#ty> {
                        let mut lock = self.#ident.lock();
                        let data = lock.take();
                        data
                    }
                });
                if is_vec {
                    constructor_tokens.push(quote! {
                        #ident: Mutex::new(Some(Vec::new())),
                    });
                    return;
                }
                constructor_tokens.push(quote! {
                    #ident: Mutex::new(None),
                });
            } else {
                if is_vec {
                    let ty = strip_single_wrapper(ty, "Vec").unwrap();
                    let push_function_ident = format_ident!("push_{}", ident);
                    let clear_function_ident = format_ident!("clear_{}", ident);

                    type_tokens.push(quote! {
                        pub fn #push_function_ident(&mut self, item: #ty) -> &#struct_ident {
                            let vec = self.#ident.as_ref();
                            if vec.is_none() {
                                self.#ident = Some(Vec::new());
                            }
                            let vec = self.#ident.as_mut().unwrap();
                            vec.push(item);
                            self
                        }

                        pub fn #clear_function_ident(&mut self) -> &#struct_ident {
                            let vec = self.#ident.as_ref();
                            if vec.is_none() {
                                self.#ident = Some(Vec::new());
                            }
                            let vec = self.#ident.as_mut().unwrap();
                            vec.clear();
                            self
                        }
                    });
                }

                type_tokens.push(quote! {
                    pub fn #set_function_ident(&mut self, #ident: #ty) -> &#struct_ident {
                        self.#ident = Some(#ident);
                        self
                    }

                    pub fn #take_function_ident(&mut self) -> Option<#ty> {
                        let data = self.#ident.take();
                        data
                    }
                });
                if is_vec {
                    constructor_tokens.push(quote! {
                        #ident: Some(Vec::new()),
                    });
                    return;
                }
                constructor_tokens.push(quote! {
                    #ident: None,
                });
            }
        } else {
            let is_vec = is_wrapper(ty, "Vec");
            if !is_vec {
                constructor_tokens.push(quote! {
                    #ident: #ident,
                });
                required_constructor_parameter_tokens.push(quote! {
                    #ident: #ty,
                });
                return;
            }
            constructor_tokens.push(quote! {
                #ident: Vec::new(),
            });

            let ty = strip_single_wrapper(ty, "Vec").unwrap();
            let push_function_ident = format_ident!("push_{}", ident);
            let clear_function_ident = format_ident!("clear_{}", ident);

            type_tokens.push(quote! {
                pub fn #push_function_ident(&mut self, item: #ty) -> &#struct_ident {
                    self.#ident.push(item);
                    self
                }

                pub fn #clear_function_ident(&mut self) -> &#struct_ident {
                    self.#ident.clear();
                    self
                }
            });
        }
    });

    let (impl_generics, ty_generics, where_clause) = a_struct.generics.split_for_impl();
    let expanded = quote! {
        #a_struct

        impl #impl_generics #struct_ident #ty_generics #where_clause {
            pub fn new(#(#required_constructor_parameter_tokens)*) -> Self {
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
