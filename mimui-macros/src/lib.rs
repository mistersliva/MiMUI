//! Syntax layer for MiMUI.
//!
//! `ui!` turns a compact token-tree DSL into ordinary method calls on `UiCtx`.
//! It deliberately does *no* semantic work: every element lowers to
//! `UiCtx::elem(tag, attrs, is_root)` followed by `UiCtx::end()`.
//!
//! Grammar (every element ends with `;`):
//!
//! ```text
//! ui! { Root {
//!     Label "hello";
//!     Button "Go" class="primary" style: "background: #6cf; padding: 8px 14px;";
//!     Column {
//!         Label "a"; Label "b";
//!     }
//! } }
//! ```
//!
//! - `Tag ... ;` — element
//! - `Tag { children }` — element with children
//! - `"literal"` — positional attribute (the text)
//! - `(expr)` — a Rust expression, converted with `to_string()`
//! - `key="value"` — string, `key=1.5` number, `key` alone → `true`
//! - `key: "value"` — sugar for `key="value"` (used by `style:`, `on_hover:` …)
//! - any string literal ending in `;` is an inline CSS declaration block
//!
//! Because the output is plain statements, ordinary Rust (`if`, `for`, `?`)
//! works directly inside the block.

use proc_macro::TokenStream;
use proc_macro2::{Delimiter, Span, TokenStream as TokenStream2, TokenTree};
use quote::quote;

/// One lowered element.
#[derive(Clone, Debug)]
struct Elem {
    tag: String,
    attrs: Vec<(String, AstAttr)>,
    children: Vec<Elem>,
    root: bool,
}

/// A single attribute value.
#[derive(Clone, Debug, PartialEq)]
enum AstAttr {
    Str(String),
    Num(f64),
    Bool(bool),
    Ident(String),
    /// An interpolated Rust expression, written `(expr)` in the DSL.
    Expr(ExprAttr),
}

/// Newtype so `Expr` can carry its own `PartialEq`.
#[derive(Clone, Debug)]
struct ExprAttr(TokenStream2);

/// Expressions compare by rendered token form; everything else structurally.
impl PartialEq for ExprAttr {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_string() == other.0.to_string()
    }
}

impl AstAttr {
    fn to_tokens(&self) -> TokenStream2 {
        match self {
            AstAttr::Str(s) => quote! { ::mimui::Attr::Str(::std::string::String::from(#s)) },
            AstAttr::Num(n) => quote! { ::mimui::Attr::Num(#n as f32) },
            AstAttr::Bool(b) => quote! { ::mimui::Attr::Bool(#b) },
            AstAttr::Ident(i) => {
                quote! { ::mimui::Attr::Ident(::std::string::String::from(#i)) }
            }
            AstAttr::Expr(e) => {
                let ts = &e.0;
                quote! { ::mimui::Attr::Text(::std::string::ToString::to_string(&(#ts))) }
            }
        }
    }
}

/// Widget tags the `ui!` macro knows about.
///
/// Only used to tell a forgotten `;` from a flag attribute: a bare name
/// followed by a literal starts a new element when the name is a tag.
const TAGS: &[&str] = &[
    "root", "mimui", "column", "row", "box", "label", "title", "button", "image", "progress",
    "checkbox", "slider", "textinput", "spacer", "divider", "col", "vstack", "hbox", "strip",
    "div", "node", "panel", "card", "text", "p", "span", "heading", "h1", "btn", "img", "icon",
    "svg", "png", "pic", "progressbar", "bar", "check", "toggle", "range", "input", "field",
    "edit", "gap", "space", "fill", "hr", "rule", "separator",
];

/// True when `name` is a widget tag.
fn is_tag(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    TAGS.contains(&lower.as_str())
}

/// What kind of token sits at a position.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Ident,
    Literal,
    Brace,
    Paren,
    Bracket,
    Punct(char),
}

fn kind(t: &TokenTree) -> Kind {
    match t {
        TokenTree::Ident(_) => Kind::Ident,
        TokenTree::Literal(_) => Kind::Literal,
        TokenTree::Group(g) => match g.delimiter() {
            Delimiter::Brace => Kind::Brace,
            Delimiter::Parenthesis => Kind::Paren,
            Delimiter::Bracket => Kind::Bracket,
            // `Delimiter::None` only appears for macro fragments, which `ui!`
            // never receives; treat it as a harmless group.
            Delimiter::None => Kind::Paren,
        },
        TokenTree::Punct(p) => Kind::Punct(p.as_char()),
    }
}

/// Parse error: a human message plus the span to point at.
type ParseErr = (String, Span);

fn fail<T: std::fmt::Display>(msg: T, span: Span) -> TokenStream {
    let msg = msg.to_string();
    quote::quote_spanned! { span => compile_error!(#msg) }.into()
}

/// The textual body of a literal: `"text"` → `text`, `12.5` → `12.5`.
fn body_of(t: &TokenTree) -> String {
    let raw = t.to_string();
    if raw.starts_with('"') {
        let s = raw.trim_matches('"');
        return s.replace("\\\"", "\"").replace("\\\\", "\\");
    }
    // Only strip a type suffix from numeric literals (`1.0f32`). A bare keyword
    // such as `true` must survive intact, so require a digit first.
    if raw.contains(|c: char| c.is_ascii_digit()) {
        return raw
            .trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '_')
            .replace('_', "");
    }
    raw
}

fn is_quoted(k: Kind, t: &TokenTree) -> bool {
    k == Kind::Literal && t.to_string().starts_with('"')
}

/// Cursor over the token stream with one token of lookahead.
struct Parser {
    toks: Vec<TokenTree>,
    pos: usize,
}

impl Parser {
    fn at(&self, n: usize) -> Option<Kind> {
        self.toks.get(self.pos + n).map(kind)
    }

    fn span(&self) -> Span {
        self.toks.get(self.pos).map_or_else(Span::call_site, TokenTree::span)
    }

    fn advance(&mut self) -> Option<TokenTree> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn ident(&mut self) -> Option<String> {
        if self.at(0) == Some(Kind::Ident) {
            self.advance().map(|t| t.to_string())
        } else {
            None
        }
    }

    fn punct(&mut self, ch: char) -> bool {
        if self.at(0) == Some(Kind::Punct(ch)) {
            self.pos += 1;
            return true;
        }
        false
    }

    fn brace_group(&mut self) -> Option<TokenStream2> {
        if self.at(0) == Some(Kind::Brace) {
            match self.advance() {
                Some(TokenTree::Group(g)) => Some(g.stream()),
                _ => None,
            }
        } else {
            None
        }
    }

    /// Optional leading `-`, then a numeric literal.
    fn number(&mut self) -> Option<f64> {
        let mut sign = 1.0;
        if self.at(0) == Some(Kind::Punct('-')) {
            // Only treat `-` as a sign when a literal follows.
            if self.at(1) != Some(Kind::Literal) {
                return None;
            }
            self.pos += 2;
            sign = -1.0;
        } else if self.at(0) == Some(Kind::Literal) {
            self.pos += 1;
        } else {
            return None;
        }
        let raw = self.toks[self.pos - 1].to_string();
        let numeric = raw
            .trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '_')
            .replace('_', "");
        numeric.parse::<f64>().ok().map(|v| v * sign)
    }
}

/// True when the token at the cursor starts a `name = value` pair.
///
/// Skips over dashes so `border-width="2px"` counts as a single key.
fn starts_pair(p: &Parser) -> bool {
    if p.at(0) != Some(Kind::Ident) {
        return false;
    }
    let mut i = 1;
    while p.at(i) == Some(Kind::Punct('-')) && p.at(i + 1) == Some(Kind::Ident) {
        i += 2;
    }
    matches!(p.at(i), Some(Kind::Punct('=')) | Some(Kind::Punct(':')))
}

fn parse_body(ts: TokenStream2) -> Result<Vec<Elem>, ParseErr> {
    let mut p = Parser { toks: ts.into_iter().collect(), pos: 0 };
    let mut out = Vec::new();

    while p.pos < p.toks.len() {
        while p.punct(';') {}
        if p.pos >= p.toks.len() {
            break;
        }

        let span = p.span();
        let tag = match p.ident() {
            Some(t) => t,
            None => {
                let found = p.toks.get(p.pos).map_or("<eof>".to_string(), ToString::to_string);
                return Err((
                    format!(
                        "MiMUI syntax: expected an element name (like `Label` or `Button`), \
                         found `{found}`. Every element must end with `;`."
                    ),
                    span,
                ));
            }
        };

        // Attributes may come before or after the children block, so scan
        // attributes first and pick up a brace group wherever it appears.
        let mut attrs = parse_attrs(&mut p, true)?;
        let children = match p.brace_group() {
            Some(inner) => {
                let inner = parse_body(inner)?;
                // Trailing attributes still belong to this element.
                attrs.extend(parse_attrs(&mut p, false)?);
                inner
            }
            None => Vec::new(),
        };

        out.push(Elem { tag, attrs, children, root: false });
    }

    Ok(out)
}

/// Parses an attribute list.
///
/// When `allow_children` is set, a brace group ends the list so the caller can
/// treat it as the element's children; that is what lets `Column gap="10px" { … }`
/// put the block after the attributes. After a block, bare flags and positional
/// values are refused, because they cannot be told apart from the next element.
fn parse_attrs(p: &mut Parser, allow_children: bool) -> Result<Vec<(String, AstAttr)>, ParseErr> {
    let allow_flags = allow_children;
    let mut attrs: Vec<(String, AstAttr)> = Vec::new();
    // The first positional value is the element's text, whatever came before it:
    // `Button id="ok" "Save"` must still read "Save".
    let mut positionals = 0usize;

    loop {
        let k = match p.at(0) {
            None => break,
            Some(k) => k,
        };

        if k == Kind::Punct(';') {
            p.pos += 1;
            break;
        }
        if allow_children && k == Kind::Brace {
            break;
        }
        if k == Kind::Punct(',') {
            p.pos += 1;
            continue;
        }

        // `key = value` / `key: value` / bare `flag`
        if k == Kind::Ident {
            // Look ahead *before* consuming: a bare name after a children
            // block belongs to the next element, not to this one.
            let is_pair = starts_pair(p);
            if !is_pair && !allow_flags {
                break;
            }

            let span = p.span();
            let start = p.pos;
            // Names may contain dashes so CSS-style keys work.
            let mut name = p.ident().expect("checked");
            while p.at(0) == Some(Kind::Punct('-')) && p.at(1) == Some(Kind::Ident) {
                p.pos += 1;
                name.push('-');
                name.push_str(&p.ident().expect("checked"));
            }
            if is_pair {
                p.pos += 1;
                let val = parse_value(p, &name)?;
                attrs.push((name, val));
                continue;
            }
            // A bare name is a flag (`disabled`) unless it starts the next element.
            // A widget tag always wins: `Box w="1px" Box w="2px"` is two boxes,
            // and stopping here keeps the first one's attributes intact.
            if is_tag(&name) {
                p.pos = start;
                break;
            }
            if p.at(0) == Some(Kind::Literal) {
                return Err((
                    format!(
                        "MiMUI syntax: `{name}` is not a property here. \
                         If you meant to start a new element, add a `;` after the previous one."
                    ),
                    span,
                ));
            }
            attrs.push((name, AstAttr::Bool(true)));
            continue;
        }

        // Positional values are ambiguous after a children block too.
        if !allow_flags {
            break;
        }

        // Positional attribute: a string, a number, or a negative number.
        let span = p.span();
        let key = if positionals == 0 { "text" } else { "css" };
        positionals += 1;
        if k == Kind::Punct('-') {
            match p.number() {
                Some(n) => {
                    attrs.push((key.to_string(), AstAttr::Num(n)));
                }
                None => return Err(("MiMUI syntax: stray `-`.".to_string(), span)),
            }
            continue;
        }

        let tok = match p.advance() {
            Some(t) => t,
            None => break,
        };
        let raw = body_of(&tok);

        if k == Kind::Paren {
            // `(expr)` interpolates a Rust expression.
            let TokenTree::Group(g) = &tok else { unreachable!("kind checked") };
            let stream = g.stream();
            attrs.push((key.to_string(), AstAttr::Expr(ExprAttr(stream))));
        } else if is_quoted(k, &tok) && raw.trim_end().ends_with(';') {
            // `"background: red; padding: 4px;"` — an inline declaration block.
            attrs.push((key.to_string(), AstAttr::Str(raw)));
        } else if is_quoted(k, &tok) {
            attrs.push((key.to_string(), AstAttr::Str(raw)));
        } else if let Ok(n) = raw.parse::<f64>() {
            attrs.push((key.to_string(), AstAttr::Num(n)));
        } else {
            return Err((format!("MiMUI syntax: unsupported literal `{raw}`."), span));
        }
    }

    Ok(attrs)
}

/// Parses the right-hand side of `key =` / `key:`.
fn parse_value(p: &mut Parser, key: &str) -> Result<AstAttr, ParseErr> {
    // `style={ ... }` — treat the group's contents as the declaration text.
    if p.at(0) == Some(Kind::Brace) {
        let inner = p.brace_group().expect("checked");
        let mut s = String::new();
        flatten(inner, &mut s);
        return Ok(AstAttr::Str(s));
    }

    // A signed number, e.g. `x=-4`.
    if p.at(0) == Some(Kind::Punct('-'))
        && p.at(1) == Some(Kind::Literal)
        && let Some(n) = p.number()
    {
        return Ok(AstAttr::Num(n));
    }

    let tok = match p.advance() {
        Some(t) => t,
        None => return Err((format!("MiMUI syntax: `{key}` has no value."), Span::call_site())),
    };
    let k = kind(&tok);
    let raw = body_of(&tok);

    if is_quoted(k, &tok) {
        return Ok(AstAttr::Str(raw));
    }
    if k == Kind::Literal
        && let Ok(n) = raw.parse::<f64>()
    {
        return Ok(AstAttr::Num(n));
    }
    match raw.as_str() {
        "true" => Ok(AstAttr::Bool(true)),
        "false" => Ok(AstAttr::Bool(false)),
        _ => Ok(AstAttr::Ident(raw)),
    }
}

/// Renders a token stream back to source-ish text (`style={...}` support).
fn flatten(ts: TokenStream2, out: &mut String) {
    for t in ts {
        match t {
            TokenTree::Group(g) => flatten(g.stream(), out),
            TokenTree::Punct(p) => out.push(p.as_char()),
            other => out.push_str(&other.to_string()),
        }
    }
}

impl Elem {
    fn emit(&self, out: &mut TokenStream2) {
        let tag = &self.tag;
        let attrs = self.attrs.iter().map(|(k, v)| {
            let toks = v.to_tokens();
            quote! { (#k, #toks) }
        });
        let attrs = quote! { ::std::vec![#(#attrs),*] };
        let root = self.root;

        out.extend(quote! { __cx.elem(#tag, #attrs, #root); });
        for child in &self.children {
            child.emit(out);
        }
        out.extend(quote! { __cx.end(); });
    }
}

/// Lowers to method calls on the `UiCtx` binding named `cx` in the caller's scope.
///
/// A lone element with children is treated as the root (`MiMUI { ... }`);
/// otherwise an implicit root wraps the list.
fn build(elems: Vec<Elem>) -> TokenStream2 {
    let mut body = TokenStream2::new();

    if elems.len() == 1 && !elems[0].children.is_empty() {
        let mut only = elems.into_iter().next().expect("len 1");
        only.root = true;
        only.emit(&mut body);
    } else {
        body.extend(quote! { __cx.elem("Root", ::std::vec![], true); });
        for e in &elems {
            e.emit(&mut body);
        }
        body.extend(quote! { __cx.end(); });
    }

    quote! {{
        ::mimui::UiCtx::with(&mut *cx, |__cx| {
            #body
        })
    }}
}

#[proc_macro]
pub fn ui(input: TokenStream) -> TokenStream {
    match parse_body(TokenStream2::from(input)) {
        Ok(elems) => build(elems).into(),
        Err((msg, span)) => fail(msg, span),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds an element list from source text.
    fn parse(src: &str) -> Result<Vec<Elem>, String> {
        parse_body(src.parse().expect("valid token stream")).map_err(|(m, _)| m)
    }

    fn names(elems: &[Elem]) -> Vec<String> {
        elems.iter().map(|e| e.tag.clone()).collect()
    }

    #[test]
    fn parses_flat_elements() {
        let elems = parse("Label \"a\"; Button \"b\";").expect("parses");
        assert_eq!(names(&elems), vec!["Label", "Button"]);
        assert_eq!(elems[0].attrs, vec![("text".into(), AstAttr::Str("a".into()))]);
    }

    #[test]
    fn parses_attributes_in_either_side_of_children() {
        let before = parse("Column gap=\"8px\" { Label \"a\"; }").expect("parses");
        assert_eq!(names(&before), vec!["Column"]);
        assert_eq!(before[0].children.len(), 1);
        assert_eq!(before[0].attrs[0], ("gap".into(), AstAttr::Str("8px".into())));

        // The same element with attributes after the block.
        let after = parse("Column { Label \"a\"; } gap=\"8px\";").expect("parses");
        assert_eq!(after[0].children.len(), 1);
        assert_eq!(after[0].attrs[0], ("gap".into(), AstAttr::Str("8px".into())));
    }

    #[test]
    fn parses_value_kinds() {
        let elems = parse("Button a=1.5 b=\"x\" c d=true e=four;").expect("parses");
        let a = &elems[0].attrs;
        assert_eq!(a[0], ("a".into(), AstAttr::Num(1.5)));
        assert_eq!(a[1], ("b".into(), AstAttr::Str("x".into())));
        // Bare names become flags; `d=true` also yields a boolean.
        assert_eq!(a[2], ("c".into(), AstAttr::Bool(true)));
        assert_eq!(a[3], ("d".into(), AstAttr::Bool(true)));
        assert_eq!(a[4], ("e".into(), AstAttr::Ident("four".into())));
    }

    #[test]
    fn parses_dashed_keys() {
        let elems = parse("Box border-color=\"#fff\" border-width=2;").expect("parses");
        assert_eq!(elems[0].attrs[0], ("border-color".into(), AstAttr::Str("#fff".into())));
        assert_eq!(elems[0].attrs[1], ("border-width".into(), AstAttr::Num(2.0)));
    }

    #[test]
    fn colon_equals_syntax() {
        let a = parse("Button x=\"1\";").expect("parses");
        let b = parse("Button x:\"1\";").expect("parses");
        assert_eq!(a[0].attrs, b[0].attrs);
    }

    #[test]
    fn first_positional_is_text_and_the_rest_are_style() {
        let elems = parse("Button \"Go\" \"background: red;\";").expect("parses");
        let a = &elems[0].attrs;
        assert_eq!(a[0], ("text".into(), AstAttr::Str("Go".into())));
        // A trailing `;` marks the second string as a CSS block.
        assert_eq!(a[1], ("css".into(), AstAttr::Str("background: red;".into())));
    }

    #[test]
    fn class_is_just_an_attribute() {
        let elems = parse("Button class=\"a b\";").expect("parses");
        assert_eq!(elems[0].attrs[0].0, "class");
    }

    #[test]
    fn negative_numbers_parse() {
        let elems = parse("Box x=-4;").expect("parses");
        assert_eq!(elems[0].attrs[0], ("x".into(), AstAttr::Num(-4.0)));
    }

    #[test]
    fn a_missing_semicolon_still_parses_when_the_name_is_a_tag() {
        // `Label "a"` followed by `Label "b"` reads as two elements.
        let elems = parse("Label \"a\" Label \"b\";").expect("parses");
        assert_eq!(names(&elems), vec!["Label", "Label"]);
        assert_eq!(elems[1].attrs[0], ("text".into(), AstAttr::Str("b".into())));
    }

#[test]
    fn a_missing_semicolon_is_reported_for_an_unknown_name() {
        // `weird` is not a tag, so this must be a property in the wrong place.
        let err = parse("Label \"a\" weird \"b\";").expect_err("should fail");
        assert!(err.contains("not a property here"), "{err}");
    }

    #[test]
    fn unknown_token_is_reported() {
        let err = parse("Label \"a\" @ 3;").expect_err("should fail");
        assert!(!err.is_empty());
    }

    #[test]
    fn nesting_is_flattened_in_order() {
        let elems = parse("Column { Label \"a\"; Row { Label \"b\"; } }").expect("parses");
        assert_eq!(names(&elems), vec!["Column"]);
        let col = &elems[0];
        assert_eq!(names(&col.children), vec!["Label", "Row"]);
        assert_eq!(names(&col.children[1].children), vec!["Label"]);
    }

    #[test]
    fn siblings_after_a_children_block_do_not_need_semicolons() {
        // A block closes an element and the next one starts immediately, which
        // is the pattern the examples use.
        let src = "Column { Row { Button \"a\"; } Column gap=\"1px\" { Label \"b\"; } }";
        let elems = parse(src).expect("parses");
        assert_eq!(names(&elems), vec!["Column"]);
        let col = &elems[0];
        assert_eq!(names(&col.children), vec!["Row", "Column"]);
        assert_eq!(col.children[1].attrs[0], ("gap".into(), AstAttr::Str("1px".into())));
    }

    #[test]
    fn parens_interpolate_an_expression() {
        let elems = parse("Label (self.title.clone());").expect("parses");
        assert_eq!(elems[0].attrs.len(), 1);
        assert_eq!(elems[0].attrs[0].0, "text");
        match &elems[0].attrs[0].1 {
            AstAttr::Expr(e) => assert!(e.0.to_string().contains("title")),
            other => panic!("expected an expression, got {other:?}"),
        }
    }

    #[test]
    fn generated_code_shape() {
        let elems = parse("MiMUI { Label \"hi\"; }").expect("parses");
        let out = build(elems).to_string();
        // The root opens, the child opens, each closes, then the root closes.
        assert_eq!(out.matches("elem").count(), 2);
        assert_eq!(out.matches("end").count(), 2);
        assert!(out.contains("\"MiMUI\""));
        assert!(out.contains("true"), "the root is flagged with `true`");
    }

    #[test]
    fn a_lone_root_becomes_the_root() {
        let out = build(parse("MiMUI { }").expect("parses")).to_string();
        assert!(out.contains("\"MiMUI\""));
    }

    #[test]
    fn a_bare_list_gets_an_implicit_root() {
        let out = build(parse("Label \"a\";").expect("parses")).to_string();
        assert!(out.contains("\"Root\""));
    }
}