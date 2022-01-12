use std::{collections::HashSet};

use proc_macro::{TokenStream, TokenTree};
use quote::{quote, ToTokens};
use syn::{Expr, ItemFn, fold::{Fold, fold_expr}, parse_macro_input, parse_quote};

lazy_static::lazy_static! {
    static ref GROUP_PATTERN: regex::Regex = {
        regex::Regex::new(r###"\[[^~`!@#$%^&*()\-+=/*{}\[\];:'"<.?]+?\]"###).unwrap()
    };
}

#[derive(PartialEq)]
enum Next
{
    Ident,
    Punct,
    Group,
}

struct Args
{
    should_mut: bool,
    has_ref: bool,
    exclude_set: HashSet<String>,
    mut_methods: Vec<String>,
}

impl Args
{
    pub fn new(metadata: TokenStream) -> Self
    {
        let mut exclude_set = HashSet::new();
        let mut mut_methods = Vec::new();

        let mut next = Next::Ident;
        let mut property = String::new();
        let mut punct = "=";

        for item in metadata
        {
            match item
            {
                TokenTree::Ident(i) =>
                {
                    if next == Next::Ident
                    {
                        property = i.to_string();
                        next = Next::Punct;
                        punct = "=";
                        continue;
                    }
                }

                TokenTree::Punct(p) =>
                {
                    if next == Next::Punct
                    {
                        if p.to_string() == punct
                        {
                            next = Next::Group;
                            continue;
                        }
                    }
                    break;
                }

                TokenTree::Group(g) =>
                {
                    if next == Next::Group
                    {
                        if let Some(cap) = GROUP_PATTERN.captures(&g.to_string())
                        {
                            let mut vaules = cap.get(0).unwrap().as_str();
                            vaules = &vaules[1..vaules.len() - 1];

                            let values = vaules.replace(" ", "");
                            for v in values.split(",")
                            {
                                match property.as_str()
                                {
                                    "exclude" =>
                                    {
                                        exclude_set.insert(v.to_string());
                                    }

                                    "mut" =>
                                    {
                                        mut_methods.push(v.to_string());
                                    }

                                    _ => {}
                                }
                            }
                        }
                        punct = ",";
                        next = Next::Punct;

                        continue;
                    }
                }

                _ =>
                {
                    break;
                }
            }
        }

        Args {
            should_mut: false,
            has_ref: false,
            exclude_set,
            mut_methods,
        }
    }
}

impl Fold for Args
{
    fn fold_expr(&mut self, i: Expr) -> Expr
    {
        match i
        {
            Expr::Index(ref ei) =>
            {
                let expr = ei.expr.clone();
                let idx = ei.index.clone();

                let name = expr.as_ref().to_token_stream().to_string();

                if self.exclude_set.is_empty() || !self.exclude_set.contains(&name)
                {
                    if self.should_mut
                    {
                        self.should_mut = false;

                        if let Expr::Range(_) = idx.as_ref()
                        {
                            if !self.has_ref
                            {
                                return Expr::from(ei.clone());
                            }

                            self.has_ref = false;

                            return parse_quote! {
                                #expr.get_unchecked_mut(#idx)
                            };
                        }
                        else
                        {
                            if self.has_ref
                            {
                                self.has_ref = false;

                                return parse_quote! {
                                    unsafe { #expr.get_unchecked_mut(#idx) }
                                };
                            }

                            return parse_quote! {
                                *#expr.get_unchecked_mut(#idx)
                            };
                        }
                    }
                    else
                    {
                        if let Expr::Range(_) = idx.as_ref()
                        {
                            if !self.has_ref
                            {
                                return Expr::from(ei.clone());
                            }

                            self.has_ref = false;

                            return parse_quote! {
                                unsafe { #expr.get_unchecked(#idx) }
                            };
                        }
                        else
                        {
                            if self.has_ref
                            {
                                self.has_ref = false;

                                return parse_quote! {
                                    unsafe { #expr.get_unchecked(#idx) }
                                };
                            }
                        
                            return parse_quote! {
                                unsafe { *#expr.get_unchecked(#idx) }
                            };
                        }
                    }
                }

                return i;
            }

            Expr::Assign(ref ea) =>
            {
                if let Expr::Index(_) = *ea.left
                {
                    let mut ea = ea.clone();
                    self.should_mut = true;

                    ea.left = Box::new(self.fold_expr(*ea.left));
                    ea.right = Box::new(self.fold_expr(*ea.right));

                    return parse_quote! {
                        unsafe { #ea }
                    };
                }
            }

            Expr::AssignOp(ref eao) =>
            {
                if let Expr::Index(_) = *eao.left
                {
                    let mut eao = eao.clone();
                    self.should_mut = true;

                    eao.left = Box::new(self.fold_expr(*eao.left));
                    eao.right = Box::new(self.fold_expr(*eao.right));

                    return parse_quote! {
                        unsafe { #eao }
                    };
                }
            }

            Expr::Reference(ref er) =>
            {
                if let Expr::Index(ei) = er.expr.as_ref()
                {
                    let name = ei.expr.as_ref().to_token_stream().to_string();
                    self.has_ref = true;

                    if self.exclude_set.is_empty() || !self.exclude_set.contains(&name)
                    {
                        if er.mutability.is_some()
                        {
                            self.should_mut = true;
                            let expr = self.fold_expr(er.expr.as_ref().clone());

                            return parse_quote! {
                                unsafe { #expr }
                            }
                        }

                        return self.fold_expr(er.expr.as_ref().clone());
                    }
                    else
                    {
                        return fold_expr(self, Expr::from(er.clone()));
                    }
                }
            }

            Expr::MethodCall(ref emc) =>
            {
                if let Expr::Index(_) = *emc.receiver
                {
                    self.has_ref = true;
                    if self.mut_methods.contains(&emc.method.to_token_stream().to_string())
                    {
                        self.should_mut = true;
                    }
                }
            }

            _ =>
            {
                return fold_expr(self, i);
            }
        }

        return fold_expr(self, i);
    }
}

#[proc_macro_attribute]
#[cfg(not(debug_assertions))]
pub fn get_unchecked(metadata: TokenStream, input: TokenStream) -> TokenStream
{
    let input_fn = parse_macro_input!(input as ItemFn);
    let mut args = Args::new(metadata);

    let output = args.fold_item_fn(input_fn);

    TokenStream::from(quote!{ #output })
}

#[proc_macro_attribute]
#[cfg(debug_assertions)]
pub fn get_unchecked(_metadata: TokenStream, input: TokenStream) -> TokenStream
{
    input
}