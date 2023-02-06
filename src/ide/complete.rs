use std::collections::{BTreeSet, HashSet};

use if_chain::if_chain;

use super::{analyze_expr, analyze_import, plain_docs_sentence, summarize_font_family};
use crate::model::{methods_on, CastInfo, Scope, Value};
use crate::syntax::{ast, LinkedNode, Source, SyntaxKind};
use crate::util::{format_eco, EcoString};
use crate::World;

/// Autocomplete a cursor position in a source file.
///
/// Returns the position from which the completions apply and a list of
/// completions.
///
/// When `explicit` is `true`, the user requested the completion by pressing
/// control and space or something similar.
pub fn autocomplete(
    world: &(dyn World + 'static),
    source: &Source,
    cursor: usize,
    explicit: bool,
) -> Option<(usize, Vec<Completion>)> {
    let mut ctx = CompletionContext::new(world, source, cursor, explicit)?;

    let _ = complete_field_accesses(&mut ctx)
        || complete_imports(&mut ctx)
        || complete_rules(&mut ctx)
        || complete_params(&mut ctx)
        || complete_markup(&mut ctx)
        || complete_math(&mut ctx)
        || complete_code(&mut ctx);

    Some((ctx.from, ctx.completions))
}

/// An autocompletion option.
#[derive(Debug, Clone)]
pub struct Completion {
    /// The kind of item this completes to.
    pub kind: CompletionKind,
    /// The label the completion is shown with.
    pub label: EcoString,
    /// The completed version of the input, possibly described with snippet
    /// syntax like `${lhs} + ${rhs}`.
    ///
    /// Should default to the `label` if `None`.
    pub apply: Option<EcoString>,
    /// An optional short description, at most one sentence.
    pub detail: Option<EcoString>,
}

/// A kind of item that can be completed.
#[derive(Debug, Clone)]
pub enum CompletionKind {
    /// A syntactical structure.
    Syntax,
    /// A function.
    Func,
    /// A function parameter.
    Param,
    /// A constant.
    Constant,
    /// A font family.
    Font,
    /// A symbol.
    Symbol(char),
}

/// Complete in markup mode.
fn complete_markup(ctx: &mut CompletionContext) -> bool {
    // Bail if we aren't even in markup.
    if !matches!(ctx.leaf.parent_kind(), None | Some(SyntaxKind::Markup)) {
        return false;
    }

    // Start of an interpolated identifier: "#|".
    if ctx.leaf.kind() == SyntaxKind::Hashtag {
        ctx.from = ctx.cursor;
        code_completions(ctx, true);
        return true;
    }

    // An existing identifier: "#pa|".
    if ctx.leaf.kind() == SyntaxKind::Ident {
        ctx.from = ctx.leaf.offset();
        code_completions(ctx, true);
        return true;
    }

    // Behind a half-completed binding: "#let x = |".
    if_chain! {
        if let Some(prev) = ctx.leaf.prev_leaf();
        if prev.kind() == SyntaxKind::Eq;
        if prev.parent_kind() == Some(SyntaxKind::LetBinding);
        then {
            ctx.from = ctx.cursor;
            code_completions(ctx, false);
            return true;
        }
    }

    // Anywhere: "|".
    if ctx.explicit {
        ctx.from = ctx.cursor;
        markup_completions(ctx);
        return true;
    }

    false
}

/// Add completions for markup snippets.
#[rustfmt::skip]
fn markup_completions(ctx: &mut CompletionContext) {
    ctx.snippet_completion(
        "expression",
        "#${}",
        "Variables, function calls, blocks, and more.",
    );

    ctx.snippet_completion(
        "linebreak",
        "\\\n${}",
        "Inserts a forced linebreak.",
    );

    ctx.snippet_completion(
        "strong text",
        "*${strong}*",
        "Strongly emphasizes content by increasing the font weight.",
    );

    ctx.snippet_completion(
        "emphasized text",
        "_${emphasized}_",
        "Emphasizes content by setting it in italic font style.",
    );

    ctx.snippet_completion(
        "raw text",
        "`${text}`",
        "Displays text verbatim, in monospace.",
    );

    ctx.snippet_completion(
        "code listing",
        "```${lang}\n${code}\n```",
        "Inserts computer code with syntax highlighting.",
    );

    ctx.snippet_completion(
        "hyperlink",
        "https://${example.com}",
        "Links to a URL.",
    );

    ctx.snippet_completion(
        "label",
        "<${name}>",
        "Makes the preceding element referencable.",
    );

    ctx.snippet_completion(
        "reference",
        "@${name}",
        "Inserts a reference to a label.",
    );

    ctx.snippet_completion(
        "heading",
        "= ${title}",
        "Inserts a section heading.",
    );

    ctx.snippet_completion(
        "list item",
        "- ${item}",
        "Inserts an item of a bullet list.",
    );

    ctx.snippet_completion(
        "enumeration item",
        "+ ${item}",
        "Inserts an item of a numbered list.",
    );

    ctx.snippet_completion(
        "enumeration item (numbered)",
        "${number}. ${item}",
        "Inserts an explicitly numbered list item.",
    );

    ctx.snippet_completion(
        "term list item",
        "/ ${term}: ${description}",
        "Inserts an item of a term list.",
    );

    ctx.snippet_completion(
        "math (inline)",
        "$${x}$",
        "Inserts an inline-level mathematical formula.",
    );

    ctx.snippet_completion(
        "math (block)",
        "$ ${sum_x^2} $",
        "Inserts a block-level mathematical formula.",
    );
}

/// Complete in math mode.
fn complete_math(ctx: &mut CompletionContext) -> bool {
    if !matches!(
        ctx.leaf.parent_kind(),
        Some(SyntaxKind::Formula)
            | Some(SyntaxKind::Math)
            | Some(SyntaxKind::MathFrac)
            | Some(SyntaxKind::MathAttach)
    ) {
        return false;
    }

    // Start of an interpolated identifier: "#|".
    if ctx.leaf.kind() == SyntaxKind::Hashtag {
        ctx.from = ctx.cursor;
        code_completions(ctx, true);
        return true;
    }

    // Behind existing atom or identifier: "$a|$" or "$abc|$".
    if matches!(ctx.leaf.kind(), SyntaxKind::Text | SyntaxKind::MathIdent) {
        ctx.from = ctx.leaf.offset();
        math_completions(ctx);
        return true;
    }

    // Anywhere: "$|$".
    if ctx.explicit {
        ctx.from = ctx.cursor;
        math_completions(ctx);
        return true;
    }

    false
}

/// Add completions for math snippets.
#[rustfmt::skip]
fn math_completions(ctx: &mut CompletionContext) {
    ctx.scope_completions(|_| true);

    ctx.snippet_completion(
        "subscript",
        "${x}_${2:2}",
        "Sets something in subscript.",
    );

    ctx.snippet_completion(
        "superscript",
        "${x}^${2:2}",
        "Sets something in superscript.",
    );

    ctx.snippet_completion(
        "fraction",
        "${x}/${y}",
        "Inserts a fraction.",
    );
}

/// Complete field accesses.
fn complete_field_accesses(ctx: &mut CompletionContext) -> bool {
    // Behind an expression plus dot: "emoji.|".
    if_chain! {
        if ctx.leaf.kind() == SyntaxKind::Dot
            || (ctx.leaf.kind() == SyntaxKind::Text
                && ctx.leaf.text() == ".");
        if ctx.leaf.range().end == ctx.cursor;
        if let Some(prev) = ctx.leaf.prev_sibling();
        if prev.is::<ast::Expr>();
        if let Some(value) = analyze_expr(ctx.world, &prev).into_iter().next();
        then {
            ctx.from = ctx.cursor;
            field_access_completions(ctx, &value);
            return true;
        }
    }

    // Behind a started field access: "emoji.fa|".
    if_chain! {
        if ctx.leaf.kind() == SyntaxKind::Ident;
        if let Some(prev) = ctx.leaf.prev_sibling();
        if prev.kind() == SyntaxKind::Dot;
        if let Some(prev_prev) = prev.prev_sibling();
        if prev_prev.is::<ast::Expr>();
        if let Some(value) = analyze_expr(ctx.world, &prev_prev).into_iter().next();
        then {
            ctx.from = ctx.leaf.offset();
            field_access_completions(ctx, &value);
            return true;
        }
    }

    false
}

/// Add completions for all fields on a value.
fn field_access_completions(ctx: &mut CompletionContext, value: &Value) {
    for &(method, args) in methods_on(value.type_name()) {
        ctx.completions.push(Completion {
            kind: CompletionKind::Func,
            label: method.into(),
            apply: Some(if args {
                format_eco!("{method}(${{}})")
            } else {
                format_eco!("{method}()${{}}")
            }),
            detail: None,
        })
    }

    match value {
        Value::Symbol(symbol) => {
            for modifier in symbol.modifiers() {
                if let Ok(modified) = symbol.clone().modified(modifier) {
                    ctx.completions.push(Completion {
                        kind: CompletionKind::Symbol(modified.get()),
                        label: modifier.into(),
                        apply: None,
                        detail: None,
                    });
                }
            }
        }
        Value::Dict(dict) => {
            for (name, value) in dict.iter() {
                ctx.value_completion(Some(name.clone().into()), value, None);
            }
        }
        Value::Module(module) => {
            for (name, value) in module.scope().iter() {
                ctx.value_completion(Some(name.clone()), value, None);
            }
        }
        _ => {}
    }
}

/// Complete imports.
fn complete_imports(ctx: &mut CompletionContext) -> bool {
    // Behind an import list:
    // "#import "path.typ": |",
    // "#import "path.typ": a, b, |".
    if_chain! {
        if let Some(prev) = ctx.leaf.prev_sibling();
        if let Some(ast::Expr::Import(import)) = prev.cast();
        if let Some(ast::Imports::Items(items)) = import.imports();
        if let Some(source) = prev.children().find(|child| child.is::<ast::Expr>());
        if let Some(value) = analyze_expr(ctx.world, &source).into_iter().next();
        then {
            ctx.from = ctx.cursor;
            import_completions(ctx, &items, &value);
            return true;
        }
    }

    // Behind a half-started identifier in an import list:
    // "#import "path.typ": thi|",
    if_chain! {
        if ctx.leaf.kind() == SyntaxKind::Ident;
        if let Some(parent) = ctx.leaf.parent();
        if parent.kind() == SyntaxKind::ImportItems;
        if let Some(grand) = parent.parent();
        if let Some(ast::Expr::Import(import)) = grand.cast();
        if let Some(ast::Imports::Items(items)) = import.imports();
        if let Some(source) = grand.children().find(|child| child.is::<ast::Expr>());
        if let Some(value) = analyze_expr(ctx.world, &source).into_iter().next();
        then {
            ctx.from = ctx.leaf.offset();
            import_completions(ctx, &items, &value);
            return true;
        }
    }

    false
}

/// Add completions for all exports of a module.
fn import_completions(
    ctx: &mut CompletionContext,
    existing: &[ast::Ident],
    value: &Value,
) {
    let module = match value {
        Value::Str(path) => match analyze_import(ctx.world, ctx.source, path) {
            Some(module) => module,
            None => return,
        },
        Value::Module(module) => module.clone(),
        _ => return,
    };

    if existing.is_empty() {
        ctx.snippet_completion("*", "*", "Import everything.");
    }

    for (name, value) in module.scope().iter() {
        if existing.iter().all(|ident| ident.as_str() != name) {
            ctx.value_completion(Some(name.clone()), value, None);
        }
    }
}

/// Complete set and show rules.
fn complete_rules(ctx: &mut CompletionContext) -> bool {
    // We don't want to complete directly behind the keyword.
    if !ctx.leaf.kind().is_trivia() {
        return false;
    }

    let Some(prev) = ctx.leaf.prev_leaf() else { return false };

    // Behind the set keyword: "set |".
    if matches!(prev.kind(), SyntaxKind::Set) {
        ctx.from = ctx.cursor;
        set_rule_completions(ctx);
        return true;
    }

    // Behind the show keyword: "show |".
    if matches!(prev.kind(), SyntaxKind::Show) {
        ctx.from = ctx.cursor;
        show_rule_selector_completions(ctx);
        return true;
    }

    // Behind a half-completed show rule: "show strong: |".
    if_chain! {
        if let Some(prev) = ctx.leaf.prev_leaf();
        if matches!(prev.kind(), SyntaxKind::Colon);
        if matches!(prev.parent_kind(), Some(SyntaxKind::ShowRule));
        then {
            ctx.from = ctx.cursor;
            show_rule_recipe_completions(ctx);
            return true;
        }
    }

    false
}

/// Add completions for all functions from the global scope.
fn set_rule_completions(ctx: &mut CompletionContext) {
    ctx.scope_completions(|value| {
        matches!(
            value,
            Value::Func(func) if func.info().map_or(false, |info| {
                info.params.iter().any(|param| param.settable)
            }),
        )
    });
}

/// Add completions for selectors.
fn show_rule_selector_completions(ctx: &mut CompletionContext) {
    ctx.scope_completions(
        |value| matches!(value, Value::Func(func) if func.select(None).is_ok()),
    );

    ctx.enrich("", ": ");

    ctx.snippet_completion(
        "text selector",
        "\"${text}\": ${}",
        "Replace occurances of specific text.",
    );

    ctx.snippet_completion(
        "regex selector",
        "regex(\"${regex}\"): ${}",
        "Replace matches of a regular expression.",
    );
}

/// Add completions for recipes.
fn show_rule_recipe_completions(ctx: &mut CompletionContext) {
    ctx.snippet_completion(
        "replacement",
        "[${content}]",
        "Replace the selected element with content.",
    );

    ctx.snippet_completion(
        "replacement (string)",
        "\"${text}\"",
        "Replace the selected element with a string of text.",
    );

    ctx.snippet_completion(
        "transformation",
        "element => [${content}]",
        "Transform the element with a function.",
    );

    ctx.scope_completions(|value| matches!(value, Value::Func(_)));
}

/// Complete call and set rule parameters.
fn complete_params(ctx: &mut CompletionContext) -> bool {
    // Ensure that we are in a function call or set rule's argument list.
    let (callee, set, args) = if_chain! {
        if let Some(parent) = ctx.leaf.parent();
        if let Some(parent) = match parent.kind() {
            SyntaxKind::Named => parent.parent(),
            _ => Some(parent),
        };
        if let Some(args) = parent.cast::<ast::Args>();
        if let Some(grand) = parent.parent();
        if let Some(expr) = grand.cast::<ast::Expr>();
        let set = matches!(expr, ast::Expr::Set(_));
        if let Some(ast::Expr::Ident(callee)) = match expr {
            ast::Expr::FuncCall(call) => Some(call.callee()),
            ast::Expr::Set(set) => Some(set.target()),
            _ => None,
        };
        then {
            (callee, set, args)
        } else {
            return false;
        }
    };

    // Parameter values: "func(param:|)", "func(param: |)".
    if_chain! {
        if let Some(prev) = ctx.leaf.prev_leaf();
        if let Some(before_colon) = match (prev.kind(), ctx.leaf.kind()) {
            (_, SyntaxKind::Colon) => Some(prev),
            (SyntaxKind::Colon, _) => prev.prev_leaf(),
            _ => None,
        };
        if let Some(param) = before_colon.cast::<ast::Ident>();
        then {
            ctx.from = match ctx.leaf.kind() {
                SyntaxKind::Colon | SyntaxKind::Space  => ctx.cursor,
                _ => ctx.leaf.offset(),
            };
            named_param_value_completions(ctx, &callee, &param);
            return true;
        }
    }

    // Parameters: "func(|)", "func(hi|)", "func(12,|)".
    if_chain! {
        if let Some(deciding) = if ctx.leaf.kind().is_trivia() {
            ctx.leaf.prev_leaf()
        } else {
            Some(ctx.leaf.clone())
        };
        if matches!(
            deciding.kind(),
            SyntaxKind::LeftParen
                | SyntaxKind::Comma
                | SyntaxKind::Ident
        );
        then {
            ctx.from = match deciding.kind() {
                SyntaxKind::Ident => deciding.offset(),
                _ => ctx.cursor,
            };

            // Exclude arguments which are already present.
            let exclude: Vec<_> = args.items().filter_map(|arg| match arg {
                ast::Arg::Named(named) => Some(named.name()),
                _ => None,
            }).collect();

            param_completions(ctx, &callee, set, &exclude);
            return true;
        }
    }

    false
}

/// Add completions for the parameters of a function.
fn param_completions(
    ctx: &mut CompletionContext,
    callee: &ast::Ident,
    set: bool,
    exclude: &[ast::Ident],
) {
    let info = if_chain! {
        if let Some(Value::Func(func)) = ctx.global.get(callee);
        if let Some(info) = func.info();
        then { info }
        else { return; }
    };

    if callee.as_str() == "text" {
        ctx.font_completions();
    }

    for param in &info.params {
        if exclude.iter().any(|ident| ident.as_str() == param.name) {
            continue;
        }

        if set && !param.settable {
            continue;
        }

        if param.named {
            ctx.completions.push(Completion {
                kind: CompletionKind::Param,
                label: param.name.into(),
                apply: Some(format_eco!("{}: ${{}}", param.name)),
                detail: Some(plain_docs_sentence(param.docs).into()),
            });
        }

        if param.positional {
            ctx.cast_completions(&param.cast);
        }
    }

    if ctx.before.ends_with(',') {
        ctx.enrich(" ", "");
    }
}

/// Add completions for the values of a named function parameter.
fn named_param_value_completions(
    ctx: &mut CompletionContext,
    callee: &ast::Ident,
    name: &str,
) {
    let param = if_chain! {
        if let Some(Value::Func(func)) = ctx.global.get(callee);
        if let Some(info) = func.info();
        if let Some(param) = info.param(name);
        if param.named;
        then { param }
        else { return; }
    };

    ctx.cast_completions(&param.cast);

    if ctx.before.ends_with(':') {
        ctx.enrich(" ", "");
    }
}

/// Complete in code mode.
fn complete_code(ctx: &mut CompletionContext) -> bool {
    if matches!(
        ctx.leaf.parent_kind(),
        None | Some(SyntaxKind::Markup)
            | Some(SyntaxKind::Math)
            | Some(SyntaxKind::MathFrac)
            | Some(SyntaxKind::MathAttach)
    ) {
        return false;
    }

    // An existing identifier: "{ pa| }".
    if ctx.leaf.kind() == SyntaxKind::Ident {
        ctx.from = ctx.leaf.offset();
        code_completions(ctx, false);
        return true;
    }

    // Anywhere: "{ | }".
    // But not within or after an expression.
    if ctx.explicit
        && (ctx.leaf.kind().is_trivia()
            || matches!(ctx.leaf.kind(), SyntaxKind::LeftParen | SyntaxKind::LeftBrace))
    {
        ctx.from = ctx.cursor;
        code_completions(ctx, false);
        return true;
    }

    false
}

/// Add completions for expression snippets.
#[rustfmt::skip]
fn code_completions(ctx: &mut CompletionContext, hashtag: bool) {
    ctx.scope_completions(|value| !hashtag || {
        matches!(value, Value::Symbol(_) | Value::Func(_) | Value::Module(_))
    });

    ctx.snippet_completion(
        "function call",
        "${function}(${arguments})[${body}]",
        "Evaluates a function.",
    );

    ctx.snippet_completion(
        "code block",
        "{ ${} }",
        "Inserts a nested code block.",
    );

    ctx.snippet_completion(
        "content block",
        "[${content}]",
        "Switches into markup mode.",
    );

    ctx.snippet_completion(
        "set rule",
        "set ${}",
        "Sets style properties on an element.",
    );

    ctx.snippet_completion(
        "show rule",
        "show ${}",
        "Redefines the look of an element.",
    );

    ctx.snippet_completion(
        "let binding",
        "let ${name} = ${value}",
        "Saves a value in a variable.",
    );

    ctx.snippet_completion(
        "let binding (function)",
        "let ${name}(${params}) = ${output}",
        "Defines a function.",
    );

    ctx.snippet_completion(
        "if conditional",
        "if ${1 < 2} {\n\t${}\n}",
        "Computes or inserts something conditionally.",
    );

    ctx.snippet_completion(
        "if-else conditional",
        "if ${1 < 2} {\n\t${}\n} else {\n\t${}\n}",
        "Computes or inserts different things based on a condition.",
    );

    ctx.snippet_completion(
        "while loop",
        "while ${1 < 2} {\n\t${}\n}",
        "Computes or inserts somthing while a condition is met.",
    );

    ctx.snippet_completion(
        "for loop",
        "for ${value} in ${(1, 2, 3)} {\n\t${}\n}",
        "Computes or inserts somthing for each value in a collection.",
    );

    ctx.snippet_completion(
        "for loop (with key)",
        "for ${key}, ${value} in ${(a: 1, b: 2)} {\n\t${}\n}",
        "Computes or inserts somthing for each key and value in a collection.",
    );

    ctx.snippet_completion(
        "break",
        "break",
        "Exits early from a loop.",
    );

    ctx.snippet_completion(
        "continue",
        "continue",
        "Continues with the next iteration of a loop.",
    );

    ctx.snippet_completion(
        "return",
        "return ${output}",
        "Returns early from a function.",
    );

    ctx.snippet_completion(
        "import",
        "import \"${file.typ}\": ${items}",
        "Imports variables from another file.",
    );

    ctx.snippet_completion(
        "include",
        "include \"${file.typ}\"",
        "Includes content from another file.",
    );

    ctx.snippet_completion(
        "array",
        "(${1, 2, 3})",
        "Creates a sequence of values.",
    );

    ctx.snippet_completion(
        "dictionary",
        "(${a: 1, b: 2})",
        "Creates a mapping from names to value.",
    );

    if !hashtag {
        ctx.snippet_completion(
            "function",
            "(${params}) => ${output}",
            "Creates an unnamed function.",
        );
    }
}

/// Context for autocompletion.
struct CompletionContext<'a> {
    world: &'a (dyn World + 'static),
    source: &'a Source,
    global: &'a Scope,
    math: &'a Scope,
    before: &'a str,
    leaf: LinkedNode<'a>,
    cursor: usize,
    explicit: bool,
    from: usize,
    completions: Vec<Completion>,
    seen_casts: HashSet<u128>,
}

impl<'a> CompletionContext<'a> {
    /// Create a new autocompletion context.
    fn new(
        world: &'a (dyn World + 'static),
        source: &'a Source,
        cursor: usize,
        explicit: bool,
    ) -> Option<Self> {
        let text = source.text();
        let leaf = LinkedNode::new(source.root()).leaf_at(cursor)?;
        Some(Self {
            world,
            source,
            global: &world.library().global.scope(),
            math: &world.library().math.scope(),
            before: &text[..cursor],
            leaf,
            cursor,
            explicit,
            from: cursor,
            completions: vec![],
            seen_casts: HashSet::new(),
        })
    }

    /// Add a prefix and suffix to all applications.
    fn enrich(&mut self, prefix: &str, suffix: &str) {
        for Completion { label, apply, .. } in &mut self.completions {
            let current = apply.as_ref().unwrap_or(label);
            *apply = Some(format_eco!("{prefix}{current}{suffix}"));
        }
    }

    /// Add a snippet completion.
    fn snippet_completion(
        &mut self,
        label: &'static str,
        snippet: &'static str,
        docs: &'static str,
    ) {
        self.completions.push(Completion {
            kind: CompletionKind::Syntax,
            label: label.into(),
            apply: Some(snippet.into()),
            detail: Some(docs.into()),
        });
    }

    /// Add completions for all font families.
    fn font_completions(&mut self) {
        for (family, iter) in self.world.book().families() {
            let detail = summarize_font_family(iter);
            self.completions.push(Completion {
                kind: CompletionKind::Font,
                label: family.into(),
                apply: Some(format_eco!("\"{family}\"")),
                detail: Some(detail.into()),
            })
        }
    }

    /// Add a completion for a specific value.
    fn value_completion(
        &mut self,
        label: Option<EcoString>,
        value: &Value,
        docs: Option<&'static str>,
    ) {
        let mut label = label.unwrap_or_else(|| value.repr().into());
        let mut apply = None;

        if label.starts_with('"') {
            let trimmed = label.trim_matches('"').into();
            apply = Some(label);
            label = trimmed;
        }

        let detail = docs.map(Into::into).or_else(|| match value {
            Value::Symbol(_) => None,
            Value::Content(_) => None,
            Value::Func(func) => {
                func.info().map(|info| plain_docs_sentence(info.docs).into())
            }
            v => Some(v.repr().into()),
        });

        self.completions.push(Completion {
            kind: match value {
                Value::Func(_) => CompletionKind::Func,
                Value::Symbol(s) => CompletionKind::Symbol(s.get()),
                _ => CompletionKind::Constant,
            },
            label,
            apply,
            detail,
        });
    }

    /// Add completions for a castable.
    fn cast_completions(&mut self, cast: &'a CastInfo) {
        // Prevent duplicate completions from appearing.
        if !self.seen_casts.insert(crate::util::hash128(cast)) {
            return;
        }

        match cast {
            CastInfo::Any => {}
            CastInfo::Value(value, docs) => {
                self.value_completion(None, value, Some(docs));
            }
            CastInfo::Type("none") => self.snippet_completion("none", "none", "Nothing."),
            CastInfo::Type("auto") => {
                self.snippet_completion("auto", "auto", "A smart default.");
            }
            CastInfo::Type("boolean") => {
                self.snippet_completion("false", "false", "No / Disabled.");
                self.snippet_completion("true", "true", "Yes / Enabled.");
            }
            CastInfo::Type("color") => {
                self.snippet_completion(
                    "luma()",
                    "luma(${v})",
                    "A custom grayscale color.",
                );
                self.snippet_completion(
                    "rgb()",
                    "rgb(${r}, ${g}, ${b}, ${a})",
                    "A custom RGBA color.",
                );
                self.snippet_completion(
                    "cmyk()",
                    "cmyk(${c}, ${m}, ${y}, ${k})",
                    "A custom CMYK color.",
                );
                self.scope_completions(|value| value.type_name() == "color");
            }
            CastInfo::Type("function") => {
                self.snippet_completion(
                    "function",
                    "(${params}) => ${output}",
                    "A custom function.",
                );
            }
            CastInfo::Type(ty) => {
                self.completions.push(Completion {
                    kind: CompletionKind::Syntax,
                    label: (*ty).into(),
                    apply: Some(format_eco!("${{{ty}}}")),
                    detail: Some(format_eco!("A value of type {ty}.")),
                });
                self.scope_completions(|value| value.type_name() == *ty);
            }
            CastInfo::Union(union) => {
                for info in union {
                    self.cast_completions(info);
                }
            }
        }
    }

    /// Add completions for definitions that are available at the cursor.
    /// Filters the global/math scope with the given filter.
    fn scope_completions(&mut self, filter: impl Fn(&Value) -> bool) {
        let mut defined = BTreeSet::new();

        let mut ancestor = Some(self.leaf.clone());
        while let Some(node) = &ancestor {
            let mut sibling = Some(node.clone());
            while let Some(node) = &sibling {
                if let Some(v) = node.cast::<ast::LetBinding>() {
                    defined.insert(v.binding().take());
                }
                sibling = node.prev_sibling();
            }

            if let Some(parent) = node.parent() {
                if let Some(v) = parent.cast::<ast::ForLoop>() {
                    if node.prev_sibling_kind() != Some(SyntaxKind::In) {
                        let pattern = v.pattern();
                        if let Some(key) = pattern.key() {
                            defined.insert(key.take());
                        }
                        defined.insert(pattern.value().take());
                    }
                }

                ancestor = Some(parent.clone());
                continue;
            }

            break;
        }

        let in_math = matches!(
            self.leaf.parent_kind(),
            Some(SyntaxKind::Formula)
                | Some(SyntaxKind::Math)
                | Some(SyntaxKind::MathFrac)
                | Some(SyntaxKind::MathAttach)
        );

        let scope = if in_math { self.math } else { self.global };
        for (name, value) in scope.iter() {
            if filter(value) && !defined.contains(name) {
                self.value_completion(Some(name.clone()), value, None);
            }
        }

        for name in defined {
            if !name.is_empty() {
                self.completions.push(Completion {
                    kind: CompletionKind::Constant,
                    label: name,
                    apply: None,
                    detail: None,
                });
            }
        }
    }
}