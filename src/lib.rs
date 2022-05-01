use std::collections::HashSet;

use proc_macro::{TokenStream, TokenTree};
use quote::{quote, ToTokens};
use syn::{Expr, ItemFn, fold::{Fold, fold_expr, fold_block}, parse_macro_input, parse_quote};

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
    outer: bool,

    has_ref_stack: Vec<bool>,
    should_mut_stack: Vec<bool>,
}

impl Args
{
    pub fn new(metadata: TokenStream) -> Self
    {
        let mut exclude_set = HashSet::new();
        let mut mut_methods = Vec::new();

        let mut next = Next::Ident;
        let mut property = String::new();

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
                        continue;
                    }
                }

                TokenTree::Punct(p) =>
                {
                    if next == Next::Punct
                    {
                        if p.to_string() == "="
                        {
                            next = Next::Group;
                            continue;
                        }
                        else if p.to_string() == ","
                        {
                            next = Next::Ident;
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
                            let mut values = cap.get(0).unwrap().as_str();
                            values = &values[1..values.len() - 1];

                            let values = values.replace(" ", "");

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
            outer: true,

            has_ref_stack: Vec::new(),
            should_mut_stack: Vec::new(),
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
                if self.has_ref
                {
                    self.has_ref_stack.push(true);
                    self.has_ref = false;
                }
                else
                {
                    self.has_ref_stack.push(false);
                }

                if self.should_mut
                {
                    self.should_mut = false;
                    self.should_mut_stack.push(true);
                }
                else
                {
                    self.should_mut_stack.push(false);
                }

                let expr = ei.expr.clone();
                let idx = ei.index.clone();

                let name = expr.as_ref().to_token_stream().to_string();
                let invoke_method: Expr;

                let expr = self.fold_expr(*expr);
                let idx = self.fold_expr(*idx);

                let has_ref = self.has_ref_stack.pop().unwrap();
                let should_mut = self.should_mut_stack.pop().unwrap();

                if self.exclude_set.is_empty() || !self.exclude_set.contains(&name)
                {
                    if should_mut
                    {
                        // self.should_mut = false;
                        invoke_method = parse_quote! { get_unchecked_mut };
                    }
                    else
                    {
                        invoke_method = parse_quote! { get_unchecked };
                    }

                    if let Expr::Range(ref er) = idx
                    {
                        let mut from: Option<Expr> = None;
                        let mut to: Option<Expr> = None;

                        if let Some(f) = er.from.clone()
                        {
                            from = Some(fold_expr(self, *f));
                        }

                        if let Some(t) = er.to.clone()
                        {
                            to = Some(fold_expr(self, *t));
                        }

                        // If only arr[i..j] (not &(mut) arr[i..j]) then ignore it
                        if !has_ref
                        {
                            return Expr::from(ei.clone());
                        }

                        let mut idx: Expr = parse_quote! {..};

                        if let Some(f) = from
                        {
                            idx = parse_quote! { #f #idx };
                        }

                        if let Some(t) = to
                        {
                            idx = parse_quote! { #idx #t };
                        }

                        return parse_quote! {
                            #expr.#invoke_method(#idx)
                        };
                    }
                    else
                    {
                        if has_ref
                        {
                            // self.has_ref = false;

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
                    self.has_ref = true;

                    if self.exclude_set.is_empty() || !self.exclude_set.contains(&name)
                    {
                        if er.mutability.is_some()
                        {
                            self.should_mut = true;
                        }

                        return self.fold_expr(er.expr.as_ref().clone());
                    }
                    else
                    {
                        return fold_expr(self, Expr::from(er.clone()));
                    }
                }
            }

            Expr::Assign(ref ea) =>
            {
                if let Expr::Index(left) = *ea.left.clone()
                {
                    self.has_ref = false;
                    self.should_mut = true;

                    let left = self.fold_expr(Expr::from(left));
                    let mut result = ea.clone();

                    let right = self.fold_expr(Expr::from(*ea.right.clone()));

                    result.left = Box::new(left);
                    result.right = Box::new(right);

                    return Expr::from(result);
                }
            }

            Expr::AssignOp(ref eao) =>
            {
                if let Expr::Index(left) = *eao.left.clone()
                {
                    self.has_ref = false;
                    self.should_mut = true;

                    let left = self.fold_expr(Expr::from(left));
                    let mut result = eao.clone();

                    let right = self.fold_expr(Expr::from(*eao.right.clone()));

                    result.left = Box::new(left);
                    result.right = Box::new(right);

                    return Expr::from(result);
                }
            }

            Expr::MethodCall(ref emc) =>
            {
                if emc.method.to_string() == "unwrap"
                {
                    if !self.exclude_set.contains(&emc.receiver.to_token_stream().to_string())
                    {
                        let mut emc = emc.clone();
                        emc.method = parse_quote! { unwrap_unchecked };

                        return Expr::from(emc);
                    }
                }
                else if let Expr::Index(_) = *emc.receiver
                {
                    println!("method call after index");
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
// #[cfg(not(debug_assertions))]
pub fn unchecked(metadata: TokenStream, input: TokenStream) -> TokenStream
{
    let input_fn = parse_macro_input!(input as ItemFn);
    let mut args = Args::new(metadata);

    let output = args.fold_item_fn(input_fn);
    println!("{}", output.to_token_stream().to_string());

    TokenStream::from(quote!{ #output })
}

// #[proc_macro_attribute]
// #[cfg(debug_assertions)]
// pub fn unchecked(_metadata: TokenStream, input: TokenStream) -> TokenStream
// {
//     input
// }