// Copyright 2018 Guillaume Pinot (@TeXitoi) <texitoi@texitoi.eu>,
// Kevin Knapp (@kbknapp) <kbknapp@gmail.com>, and
// Ana Hobden (@hoverbear) <operator@hoverbear.org>
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.
//
// This work was derived from Structopt (https://github.com/TeXitoi/structopt)
// commit#ea76fa1b1b273e65e3b0b1046643715b49bec51f which is licensed under the
// MIT/Apache 2.0 license.

use proc_macro2::{Ident, Span, TokenStream};
use proc_macro_error::{abort, abort_call_site};
use quote::{format_ident, quote, quote_spanned};
use syn::{spanned::Spanned, Data, DeriveInput, FieldsUnnamed, Generics, Variant};

use crate::derives::args;
use crate::dummies;
use crate::item::{Item, Kind, Name};
use crate::utils::{is_simple_ty, subty_if_name};

pub fn derive_subcommand(input: &DeriveInput) -> TokenStream {
    let ident = &input.ident;

    dummies::subcommand(ident);

    match input.data {
        Data::Enum(ref e) => {
            let name = Name::Derived(ident.clone());
            let item = Item::from_subcommand_enum(input, name);
            let variants = e
                .variants
                .iter()
                .map(|variant| {
                    let item =
                        Item::from_subcommand_variant(variant, item.casing(), item.env_casing());
                    (variant, item)
                })
                .collect::<Vec<_>>();
            gen_for_enum(&item, ident, &input.generics, &variants)
        }
        _ => abort_call_site!("`#[derive(Subcommand)]` only supports enums"),
    }
}

pub fn gen_for_enum(
    item: &Item,
    item_name: &Ident,
    generics: &Generics,
    variants: &[(&Variant, Item)],
) -> TokenStream {
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let from_arg_matches = gen_from_arg_matches(variants);
    let update_from_arg_matches = gen_update_from_arg_matches(variants);

    let augmentation = gen_augment(variants, item, false);
    let augmentation_update = gen_augment(variants, item, true);
    let has_subcommand = gen_has_subcommand(variants);

    quote! {
        #[allow(dead_code, unreachable_code, unused_variables, unused_braces)]
        #[allow(
            clippy::style,
            clippy::complexity,
            clippy::pedantic,
            clippy::restriction,
            clippy::perf,
            clippy::deprecated,
            clippy::nursery,
            clippy::cargo,
            clippy::suspicious_else_formatting,
        )]
        #[deny(clippy::correctness)]
        impl #impl_generics clap::FromArgMatches for #item_name #ty_generics #where_clause {
            fn from_arg_matches(__clap_arg_matches: &clap::ArgMatches) -> ::std::result::Result<Self, clap::Error> {
                Self::from_arg_matches_mut(&mut __clap_arg_matches.clone())
            }

            #from_arg_matches

            fn update_from_arg_matches(&mut self, __clap_arg_matches: &clap::ArgMatches) -> ::std::result::Result<(), clap::Error> {
                self.update_from_arg_matches_mut(&mut __clap_arg_matches.clone())
            }
            #update_from_arg_matches
        }

        #[allow(dead_code, unreachable_code, unused_variables, unused_braces)]
        #[allow(
            clippy::style,
            clippy::complexity,
            clippy::pedantic,
            clippy::restriction,
            clippy::perf,
            clippy::deprecated,
            clippy::nursery,
            clippy::cargo,
            clippy::suspicious_else_formatting,
        )]
        #[deny(clippy::correctness)]
        impl #impl_generics clap::Subcommand for #item_name #ty_generics #where_clause {
            fn augment_subcommands <'b>(__clap_app: clap::Command) -> clap::Command {
                #augmentation
            }
            fn augment_subcommands_for_update <'b>(__clap_app: clap::Command) -> clap::Command {
                #augmentation_update
            }
            fn has_subcommand(__clap_name: &str) -> bool {
                #has_subcommand
            }
        }
    }
}

fn gen_augment(
    variants: &[(&Variant, Item)],
    parent_item: &Item,
    override_required: bool,
) -> TokenStream {
    use syn::Fields::*;

    let app_var = Ident::new("__clap_app", Span::call_site());

    let subcommands: Vec<_> = variants
        .iter()
        .filter_map(|(variant, item)| {
            let kind = item.kind();

            match &*kind {
                Kind::Skip(_) => None,

                Kind::ExternalSubcommand => {
                    let ty = match variant.fields {
                        Unnamed(ref fields) if fields.unnamed.len() == 1 => &fields.unnamed[0].ty,

                        _ => abort!(
                            variant,
                            "The enum variant marked with `external_subcommand` must be \
                             a single-typed tuple, and the type must be either `Vec<String>` \
                             or `Vec<OsString>`."
                        ),
                    };
                    let deprecations = if !override_required {
                        item.deprecations()
                    } else {
                        quote!()
                    };
                    let subcommand = match subty_if_name(ty, "Vec") {
                        Some(subty) => {
                            quote_spanned! { kind.span()=>
                                #deprecations
                                let #app_var = #app_var.external_subcommand_value_parser(clap::value_parser!(#subty));
                            }
                        }

                        None => abort!(
                            ty.span(),
                            "The type must be `Vec<_>` \
                             to be used with `external_subcommand`."
                        ),
                    };
                    Some(subcommand)
                }

                Kind::Flatten => match variant.fields {
                    Unnamed(FieldsUnnamed { ref unnamed, .. }) if unnamed.len() == 1 => {
                        let ty = &unnamed[0];
                        let deprecations = if !override_required {
                            item.deprecations()
                        } else {
                            quote!()
                        };
                        let old_heading_var = format_ident!("__clap_old_heading");
                        let next_help_heading = item.next_help_heading();
                        let next_display_order = item.next_display_order();
                        let subcommand = if override_required {
                            quote! {
                                #deprecations
                                let #old_heading_var = #app_var.get_next_help_heading().map(|s| clap::builder::Str::from(s.to_owned()));
                                let #app_var = #app_var #next_help_heading #next_display_order;
                                let #app_var = <#ty as clap::Subcommand>::augment_subcommands_for_update(#app_var);
                                let #app_var = #app_var.next_help_heading(clap::builder::Resettable::from(#old_heading_var));
                            }
                        } else {
                            quote! {
                                #deprecations
                                let #old_heading_var = #app_var.get_next_help_heading().map(|s| clap::builder::Str::from(s.to_owned()));
                                let #app_var = #app_var #next_help_heading #next_display_order;
                                let #app_var = <#ty as clap::Subcommand>::augment_subcommands(#app_var);
                                let #app_var = #app_var.next_help_heading(clap::builder::Resettable::from(#old_heading_var));
                            }
                        };
                        Some(subcommand)
                    }
                    _ => abort!(
                        variant,
                        "`flatten` is usable only with single-typed tuple variants"
                    ),
                },

                Kind::Subcommand(_) => {
                    let subcommand_var = Ident::new("__clap_subcommand", Span::call_site());
                    let arg_block = match variant.fields {
                        Named(_) => {
                            abort!(variant, "non single-typed tuple enums are not supported")
                        }
                        Unit => quote!( #subcommand_var ),
                        Unnamed(FieldsUnnamed { ref unnamed, .. }) if unnamed.len() == 1 => {
                            let ty = &unnamed[0];
                            if override_required {
                                quote_spanned! { ty.span()=>
                                    {
                                        <#ty as clap::Subcommand>::augment_subcommands_for_update(#subcommand_var)
                                    }
                                }
                            } else {
                                quote_spanned! { ty.span()=>
                                    {
                                        <#ty as clap::Subcommand>::augment_subcommands(#subcommand_var)
                                    }
                                }
                            }
                        }
                        Unnamed(..) => {
                            abort!(variant, "non single-typed tuple enums are not supported")
                        }
                    };

                    let name = item.cased_name();
                    let deprecations = if !override_required {
                        item.deprecations()
                    } else {
                        quote!()
                    };
                    let initial_app_methods = item.initial_top_level_methods();
                    let final_from_attrs = item.final_top_level_methods();
                    let subcommand = quote! {
                        let #app_var = #app_var.subcommand({
                            #deprecations;
                            let #subcommand_var = clap::Command::new(#name);
                            let #subcommand_var = #subcommand_var #initial_app_methods;
                            let #subcommand_var = #arg_block;
                            let #subcommand_var = #subcommand_var
                                .subcommand_required(true)
                                .arg_required_else_help(true);
                            #subcommand_var #final_from_attrs
                        });
                    };
                    Some(subcommand)
                }

                _ => {
                    let subcommand_var = Ident::new("__clap_subcommand", Span::call_site());
                    let sub_augment = match variant.fields {
                        Named(ref fields) => {
                            // Defer to `gen_augment` for adding cmd methods
                            let fields = fields
                                .named
                                .iter()
                                .map(|field| {
                                    let item = Item::from_args_field(field, item.casing(), item.env_casing());
                                    (field, item)
                                })
                                .collect::<Vec<_>>();
                            args::gen_augment(&fields, &subcommand_var, item, override_required)
                        }
                        Unit => {
                            let arg_block = quote!( #subcommand_var );
                            let initial_app_methods = item.initial_top_level_methods();
                            let final_from_attrs = item.final_top_level_methods();
                            quote! {
                                let #subcommand_var = #subcommand_var #initial_app_methods;
                                let #subcommand_var = #arg_block;
                                #subcommand_var #final_from_attrs
                            }
                        },
                        Unnamed(FieldsUnnamed { ref unnamed, .. }) if unnamed.len() == 1 => {
                            let ty = &unnamed[0];
                            let arg_block = if override_required {
                                quote_spanned! { ty.span()=>
                                    {
                                        <#ty as clap::Args>::augment_args_for_update(#subcommand_var)
                                    }
                                }
                            } else {
                                quote_spanned! { ty.span()=>
                                    {
                                        <#ty as clap::Args>::augment_args(#subcommand_var)
                                    }
                                }
                            };
                            let initial_app_methods = item.initial_top_level_methods();
                            let final_from_attrs = item.final_top_level_methods();
                            quote! {
                                let #subcommand_var = #subcommand_var #initial_app_methods;
                                let #subcommand_var = #arg_block;
                                #subcommand_var #final_from_attrs
                            }
                        }
                        Unnamed(..) => {
                            abort!(variant, "non single-typed tuple enums are not supported")
                        }
                    };

                    let deprecations = if !override_required {
                        item.deprecations()
                    } else {
                        quote!()
                    };
                    let name = item.cased_name();
                    let subcommand = quote! {
                        let #app_var = #app_var.subcommand({
                            #deprecations
                            let #subcommand_var = clap::Command::new(#name);
                            #sub_augment
                        });
                    };
                    Some(subcommand)
                }
            }
        })
        .collect();

    let deprecations = if !override_required {
        parent_item.deprecations()
    } else {
        quote!()
    };
    let initial_app_methods = parent_item.initial_top_level_methods();
    let final_app_methods = parent_item.final_top_level_methods();
    quote! {
        #deprecations;
        let #app_var = #app_var #initial_app_methods;
        #( #subcommands )*;
        #app_var #final_app_methods
    }
}

fn gen_has_subcommand(variants: &[(&Variant, Item)]) -> TokenStream {
    use syn::Fields::*;

    let mut ext_subcmd = false;

    let (flatten_variants, variants): (Vec<_>, Vec<_>) = variants
        .iter()
        .filter_map(|(variant, item)| {
            if let Kind::ExternalSubcommand = &*item.kind() {
                ext_subcmd = true;
                None
            } else {
                Some((variant, item))
            }
        })
        .partition(|(_, item)| {
            let kind = item.kind();
            matches!(&*kind, Kind::Flatten)
        });

    let subcommands = variants.iter().map(|(_variant, item)| {
        let sub_name = item.cased_name();
        quote! {
            if #sub_name == __clap_name {
                return true
            }
        }
    });
    let child_subcommands = flatten_variants
        .iter()
        .map(|(variant, _attrs)| match variant.fields {
            Unnamed(ref fields) if fields.unnamed.len() == 1 => {
                let ty = &fields.unnamed[0];
                quote! {
                    if <#ty as clap::Subcommand>::has_subcommand(__clap_name) {
                        return true;
                    }
                }
            }
            _ => abort!(
                variant,
                "`flatten` is usable only with single-typed tuple variants"
            ),
        });

    if ext_subcmd {
        quote! { true }
    } else {
        quote! {
            #( #subcommands )*

            #( #child_subcommands )else*

            false
        }
    }
}

fn gen_from_arg_matches(variants: &[(&Variant, Item)]) -> TokenStream {
    use syn::Fields::*;

    let mut ext_subcmd = None;

    let subcommand_name_var = format_ident!("__clap_name");
    let sub_arg_matches_var = format_ident!("__clap_arg_matches");
    let (flatten_variants, variants): (Vec<_>, Vec<_>) = variants
        .iter()
        .filter_map(|(variant, item)| {
            if let Kind::ExternalSubcommand = &*item.kind() {
                if ext_subcmd.is_some() {
                    abort!(
                        item.kind().span(),
                        "Only one variant can be marked with `external_subcommand`, \
                         this is the second"
                    );
                }

                let ty = match variant.fields {
                    Unnamed(ref fields) if fields.unnamed.len() == 1 => &fields.unnamed[0].ty,

                    _ => abort!(
                        variant,
                        "The enum variant marked with `external_subcommand` must be \
                         a single-typed tuple, and the type must be either `Vec<String>` \
                         or `Vec<OsString>`."
                    ),
                };

                let (span, str_ty) = match subty_if_name(ty, "Vec") {
                    Some(subty) => {
                        if is_simple_ty(subty, "String") {
                            (subty.span(), quote!(::std::string::String))
                        } else if is_simple_ty(subty, "OsString") {
                            (subty.span(), quote!(::std::ffi::OsString))
                        } else {
                            abort!(
                                ty.span(),
                                "The type must be either `Vec<String>` or `Vec<OsString>` \
                                 to be used with `external_subcommand`."
                            );
                        }
                    }

                    None => abort!(
                        ty.span(),
                        "The type must be either `Vec<String>` or `Vec<OsString>` \
                         to be used with `external_subcommand`."
                    ),
                };

                ext_subcmd = Some((span, &variant.ident, str_ty));
                None
            } else {
                Some((variant, item))
            }
        })
        .partition(|(_, item)| {
            let kind = item.kind();
            matches!(&*kind, Kind::Flatten)
        });

    let subcommands = variants.iter().map(|(variant, item)| {
        let sub_name = item.cased_name();
        let variant_name = &variant.ident;
        let constructor_block = match variant.fields {
            Named(ref fields) => {
                let fields = fields
                    .named
                    .iter()
                    .map(|field| {
                        let item = Item::from_args_field(field, item.casing(), item.env_casing());
                        (field, item)
                    })
                    .collect::<Vec<_>>();
                args::gen_constructor(&fields)
            },
            Unit => quote!(),
            Unnamed(ref fields) if fields.unnamed.len() == 1 => {
                let ty = &fields.unnamed[0];
                quote!( ( <#ty as clap::FromArgMatches>::from_arg_matches_mut(__clap_arg_matches)? ) )
            }
            Unnamed(..) => abort_call_site!("{}: tuple enums are not supported", variant.ident),
        };

        quote! {
            if #subcommand_name_var == #sub_name && !#sub_arg_matches_var.contains_id("") {
                return ::std::result::Result::Ok(Self :: #variant_name #constructor_block)
            }
        }
    });
    let child_subcommands = flatten_variants.iter().map(|(variant, _attrs)| {
        let variant_name = &variant.ident;
        match variant.fields {
            Unnamed(ref fields) if fields.unnamed.len() == 1 => {
                let ty = &fields.unnamed[0];
                quote! {
                    if __clap_arg_matches
                        .subcommand_name()
                        .map(|__clap_name| <#ty as clap::Subcommand>::has_subcommand(__clap_name))
                        .unwrap_or_default()
                    {
                        let __clap_res = <#ty as clap::FromArgMatches>::from_arg_matches_mut(__clap_arg_matches)?;
                        return ::std::result::Result::Ok(Self :: #variant_name (__clap_res));
                    }
                }
            }
            _ => abort!(
                variant,
                "`flatten` is usable only with single-typed tuple variants"
            ),
        }
    });

    let wildcard = match ext_subcmd {
        Some((span, var_name, str_ty)) => quote_spanned! { span=>
                ::std::result::Result::Ok(Self::#var_name(
                    ::std::iter::once(#str_ty::from(#subcommand_name_var))
                    .chain(
                        #sub_arg_matches_var
                            .remove_many::<#str_ty>("")
                            .unwrap()
                            .map(#str_ty::from)
                    )
                    .collect::<::std::vec::Vec<_>>()
                ))
        },

        None => quote! {
            ::std::result::Result::Err(clap::Error::raw(clap::error::ErrorKind::InvalidSubcommand, format!("The subcommand '{}' wasn't recognized", #subcommand_name_var)))
        },
    };

    let raw_deprecated = args::raw_deprecated();
    quote! {
        fn from_arg_matches_mut(__clap_arg_matches: &mut clap::ArgMatches) -> ::std::result::Result<Self, clap::Error> {
            #raw_deprecated

            #( #child_subcommands )else*

            if let Some((#subcommand_name_var, mut __clap_arg_sub_matches)) = __clap_arg_matches.remove_subcommand() {
                let #sub_arg_matches_var = &mut __clap_arg_sub_matches;
                #( #subcommands )*

                #wildcard
            } else {
                ::std::result::Result::Err(clap::Error::raw(clap::error::ErrorKind::MissingSubcommand, "A subcommand is required but one was not provided."))
            }
        }
    }
}

fn gen_update_from_arg_matches(variants: &[(&Variant, Item)]) -> TokenStream {
    use syn::Fields::*;

    let (flatten, variants): (Vec<_>, Vec<_>) = variants
        .iter()
        .filter_map(|(variant, item)| {
            match &*item.kind() {
                // Fallback to `from_arg_matches_mut`
                Kind::ExternalSubcommand => None,
                _ => Some((variant, item)),
            }
        })
        .partition(|(_, item)| {
            let kind = item.kind();
            matches!(&*kind, Kind::Flatten)
        });

    let subcommands = variants.iter().map(|(variant, item)| {
        let sub_name = item.cased_name();
        let variant_name = &variant.ident;
        let (pattern, updater) = match variant.fields {
            Named(ref fields) => {
                let field_names = fields.named.iter().map(|field| {
                    field.ident.as_ref().unwrap()
                }).collect::<Vec<_>>();
                let fields = fields
                    .named
                    .iter()
                    .map(|field| {
                        let item = Item::from_args_field(field, item.casing(), item.env_casing());
                        (field, item)
                    })
                    .collect::<Vec<_>>();
                let update = args::gen_updater(&fields, false);
                (quote!( { #( #field_names, )* }), quote!( { #update } ))
            }
            Unit => (quote!(), quote!({})),
            Unnamed(ref fields) => {
                if fields.unnamed.len() == 1 {
                    (
                        quote!((ref mut __clap_arg)),
                        quote!(clap::FromArgMatches::update_from_arg_matches_mut(
                            __clap_arg,
                            __clap_arg_matches
                        )?),
                    )
                } else {
                    abort_call_site!("{}: tuple enums are not supported", variant.ident)
                }
            }
        };

        quote! {
            Self :: #variant_name #pattern if #sub_name == __clap_name => {
                let (_, mut __clap_arg_sub_matches) = __clap_arg_matches.remove_subcommand().unwrap();
                let __clap_arg_matches = &mut __clap_arg_sub_matches;
                #updater
            }
        }
    });

    let child_subcommands = flatten.iter().map(|(variant, _attrs)| {
        let variant_name = &variant.ident;
        match variant.fields {
            Unnamed(ref fields) if fields.unnamed.len() == 1 => {
                let ty = &fields.unnamed[0];
                quote! {
                    if <#ty as clap::Subcommand>::has_subcommand(__clap_name) {
                        if let Self :: #variant_name (child) = s {
                            <#ty as clap::FromArgMatches>::update_from_arg_matches_mut(child, __clap_arg_matches)?;
                            return ::std::result::Result::Ok(());
                        }
                    }
                }
            }
            _ => abort!(
                variant,
                "`flatten` is usable only with single-typed tuple variants"
            ),
        }
    });

    let raw_deprecated = args::raw_deprecated();
    quote! {
        fn update_from_arg_matches_mut<'b>(
            &mut self,
            __clap_arg_matches: &mut clap::ArgMatches,
        ) -> ::std::result::Result<(), clap::Error> {
            #raw_deprecated

            if let Some(__clap_name) = __clap_arg_matches.subcommand_name() {
                match self {
                    #( #subcommands ),*
                    s => {
                        #( #child_subcommands )*
                        *s = <Self as clap::FromArgMatches>::from_arg_matches_mut(__clap_arg_matches)?;
                    }
                }
            }
            ::std::result::Result::Ok(())
        }
    }
}
