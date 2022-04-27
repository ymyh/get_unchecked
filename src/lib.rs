use std::collections::HashSet;

use proc_macro::{TokenStream, TokenTree};
use quote::{quote, ToTokens};
use syn::{Expr, ItemFn, fold::{Fold, fold_expr, fold_item_fn, fold_block}, parse_macro_input, parse_quote};

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
    should_mut: i32,
    has_ref: i32,
    exclude_set: HashSet<String>,
    mut_methods: Vec<String>,
    outer: bool,
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

                                    "unwrap_exclude" =>
                                    {
                                        exclude_set.insert(v.to_string());
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
            should_mut: 0,
            has_ref: 0,
            exclude_set,
            mut_methods,
            outer: true,
        }
    }
}

impl Fold for Args
{
    fn fold_block(&mut self, b: syn::Block) -> syn::Block
    {
        if self.outer
        {
            let mut block = b.clone();
            let stmt = b.stmts.clone();

            block.stmts = parse_quote! { unsafe { #(#stmt)* } };
            self.outer = false;
            
            return fold_block(self, block);
        }

        return fold_block(self, b);
    }

    fn fold_expr(&mut self, i: Expr) -> Expr
    {
        match i
        {
            Expr::Index(ref ei) =>
            {
                let expr = ei.expr.clone();
                let idx = ei.index.clone();

                let name = expr.as_ref().to_token_stream().to_string();
                let invoke_method: Expr;

                if self.exclude_set.is_empty() || !self.exclude_set.contains(&name)
                {
                    let idx = self.fold_expr(*idx.clone());

                    if self.should_mut != 0
                    {
                        self.should_mut -= 1;
                        invoke_method = parse_quote! { get_unchecked_mut };
                    }
                    else
                    {
                        invoke_method = parse_quote! { get_unchecked }
                    }

                    if let Expr::Range(ref er) = idx
                    {
                        // let mut from = parse_quote! {};
                        // let mut to = parse_quote! {};

                        // if let Some(f) = er.from.clone()
                        // {
                        //     from = self.fold_expr(*f);
                        // }

                        // if let Some(t) = er.to.clone()
                        // {
                        //     to = self.fold_expr(*t);
                        // }

                        // If only arr[i..j] (not &(mut) arr[i..j]) then ignore it
                        if self.has_ref == 0
                        {
                            return Expr::from(ei.clone());
                        }

                        self.has_ref -= 1;

                        return parse_quote! {
                            #expr.#invoke_method(#idx)
                        };
                    }
                    else
                    {
                        if self.has_ref != 0
                        {
                            self.has_ref -= 1;

                            return parse_quote! {
                               #expr.#invoke_method(#idx)
                            };
                        }

                        return parse_quote! {
                            *#expr.#invoke_method(#idx)
                        };
                    }
                }

                return i;
            }

            Expr::Reference(ref er) =>
            {
                if let Expr::Index(ei) = er.expr.as_ref()
                {
                    let name = ei.expr.as_ref().to_token_stream().to_string();
                    self.has_ref += 1;

                    if self.exclude_set.is_empty() || !self.exclude_set.contains(&name)
                    {
                        if er.mutability.is_some()
                        {
                            self.should_mut += 1;

                            return self.fold_expr(er.expr.as_ref().clone());
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
                if emc.method.to_string() == "unwrap"
                {
                    if !self.exclude_set.contains(&emc.receiver.to_token_stream().to_string())
                    {
                        self.fold_expr_method_call(emc.clone());
                    }
                }
                else if let Expr::Index(_) = *emc.receiver
                {
                    self.has_ref += 1;
                    if self.mut_methods.contains(&emc.method.to_token_stream().to_string())
                    {
                        self.should_mut += 1;
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

    fn fold_expr_method_call(&mut self, emc: syn::ExprMethodCall) -> syn::ExprMethodCall
    {
        let mut emc = emc.clone();
        emc.method = parse_quote! { unwrap_unchecked };

        return emc;
    }
}

#[proc_macro_attribute]
// #[cfg(not(debug_assertions))]
pub fn unchecking(metadata: TokenStream, input: TokenStream) -> TokenStream
{
    let input_fn = parse_macro_input!(input as ItemFn);
    let mut args = Args::new(metadata);

    let output = args.fold_item_fn(input_fn);
    println!("{}", output.to_token_stream().to_string());

    TokenStream::from(quote!{ #output })
}

// #[proc_macro_attribute]
// #[cfg(debug_assertions)]
// pub fn unchecking(_metadata: TokenStream, input: TokenStream) -> TokenStream
// {
//     input
// }