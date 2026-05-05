//! Search mode: find leaf items by name across the entire crate.
//!
//! Walks all items recursively, matches against a pattern (smart-case,
//! multi-word AND on full path, comma-separated OR groups), and renders
//! each match as a one-liner with a kind prefix and full path.

use std::collections::HashSet;

use rustdoc_types::{
    Attribute, Id, Item, ItemEnum, MacroKind, Struct, StructKind, Type, VariantKind, Visibility,
};

use crate::cli::FilterArgs;
use crate::model::{CrateModel, ReachableInfo, is_visible_from};
use crate::render;

/// A leaf item discovered by the walker, with its full display path and kind.
pub(crate) struct LeafItem<'a> {
    /// Full path without crate prefix, e.g. `outer::PubStruct::pub_field`
    pub(crate) path: String,
    pub(crate) item: &'a Item,
    pub(crate) kind: LeafKind,
    /// Extra rendering context (e.g. parent type name for fields/variants)
    pub(crate) context: LeafContext<'a>,
}

pub(crate) enum LeafKind {
    Function,
    Struct,
    Enum,
    Trait,
    Union,
    Field,
    Variant,
    Constant,
    Static,
    TypeAlias,
    Macro,
    ProcMacro,
    ProcAttrMacro,
    ProcDeriveMacro,
    AssocType,
    AssocConst,
    Use,
}

impl LeafKind {
    /// Match against a user-provided kind string (e.g. "fn", "struct").
    fn matches_kind_str(&self, s: &str) -> bool {
        matches!(
            (self, s),
            (LeafKind::Function, "fn")
                | (LeafKind::Struct, "struct")
                | (LeafKind::Enum, "enum")
                | (LeafKind::Trait, "trait")
                | (LeafKind::Union, "union")
                | (LeafKind::Field, "field")
                | (LeafKind::Variant, "variant")
                | (LeafKind::Constant, "const")
                | (LeafKind::Static, "static")
                | (LeafKind::TypeAlias, "type")
                | (LeafKind::Macro, "macro")
                | (LeafKind::ProcMacro, "proc-macro")
                | (LeafKind::ProcAttrMacro, "attr-macro")
                | (LeafKind::ProcDeriveMacro, "derive-macro")
                | (LeafKind::AssocType, "type")
                | (LeafKind::AssocConst, "const")
                | (LeafKind::Use, "use")
        )
    }

    /// Sort key: determines display order by kind.
    fn sort_key(&self) -> u8 {
        match self {
            LeafKind::Function => 0,
            LeafKind::Struct => 1,
            LeafKind::Enum => 2,
            LeafKind::Trait => 3,
            LeafKind::Union => 4,
            LeafKind::Field => 5,
            LeafKind::Variant => 6,
            LeafKind::Constant => 7,
            LeafKind::Static => 8,
            LeafKind::TypeAlias => 9,
            LeafKind::Macro => 10,
            LeafKind::ProcMacro => 11,
            LeafKind::ProcAttrMacro => 12,
            LeafKind::ProcDeriveMacro => 13,
            LeafKind::AssocType => 14,
            LeafKind::AssocConst => 15,
            LeafKind::Use => 16,
        }
    }
}

/// Extra context needed to render certain leaf types.
pub(crate) enum LeafContext<'a> {
    None,
    /// For fields: the parent struct
    Field {
        field_type: &'a Type,
    },
    /// For enum variants: variant kind data
    Variant,
    /// For functions in impl blocks: extra qualifiers
    ImplMethod,
    /// For associated types in impl blocks
    AssocType,
    /// For associated consts in impl blocks
    AssocConst,
    /// For re-exports: the source path
    Use {
        source: String,
    },
}

/// Check if a leaf item is a member of a parent type (field, variant, method, assoc item).
///
/// Free functions (context = None) are NOT members. Impl methods (context = ImplMethod) are.
fn is_member(leaf: &LeafItem) -> bool {
    matches!(
        leaf.kind,
        LeafKind::Field | LeafKind::Variant | LeafKind::AssocType | LeafKind::AssocConst
    ) || matches!(
        (&leaf.kind, &leaf.context),
        (LeafKind::Function, LeafContext::ImplMethod)
    )
}

/// Parse `--search-limit` value: `"N"` → (0, Some(N)), `"M:N"` → (M, Some(N)), `None` → (0, None).
fn parse_search_limit(raw: Option<&str>) -> (usize, Option<usize>) {
    let Some(raw) = raw else {
        return (0, None);
    };
    if let Some((offset_str, limit_str)) = raw.split_once(':') {
        let offset = offset_str.parse().unwrap_or(0);
        let limit = limit_str.parse().unwrap_or(0);
        (offset, Some(limit))
    } else {
        let limit = raw.parse().unwrap_or(0);
        (0, Some(limit))
    }
}

// === Pattern DSL ===

/// A parsed token kind.
enum TokenKind {
    /// `path.contains(s)` — bare word, no wildcards
    Substring(String),
    /// `glob_match(pat, path)` — token contains `*` or `?`
    Glob(String),
    /// `path.rsplit("::").next() == Some(name)` — `=term` syntax
    Exact(String),
}

/// Parsed result of the full pattern string.
struct ParsedPattern {
    /// OR groups of positive (include) tokens. Each group is AND-matched.
    or_groups: Vec<Vec<TokenKind>>,
    /// Exclusion tokens from all groups, applied as global post-filter.
    exclusions: Vec<TokenKind>,
}

/// Parse a pattern string into structured tokens.
///
/// 1. Split by `,` → OR groups.
/// 2. Each group: split by whitespace → raw tokens.
/// 3. Per token: strip `-` prefix → exclusion. Strip `=` prefix → Exact.
///    Contains `*`/`?` → Glob. Else → Substring.
/// 4. Apply case normalization if `!case_sensitive`.
fn parse_pattern(pattern: &str, case_sensitive: bool) -> ParsedPattern {
    let mut or_groups: Vec<Vec<TokenKind>> = Vec::new();
    let mut exclusions: Vec<TokenKind> = Vec::new();

    for group_str in pattern.split(',') {
        let mut group_tokens: Vec<TokenKind> = Vec::new();

        for raw_token in group_str.split_whitespace() {
            let (is_exclude, rest) = if let Some(rest) = raw_token.strip_prefix('-') {
                (true, rest)
            } else {
                (false, raw_token)
            };

            // Skip empty remainder (bare `-` or bare `=`)
            if rest.is_empty() {
                continue;
            }

            let token = if let Some(name) = rest.strip_prefix('=') {
                if name.is_empty() {
                    continue;
                }
                let name = if case_sensitive {
                    name.to_string()
                } else {
                    name.to_lowercase()
                };
                TokenKind::Exact(name)
            } else if rest.contains('*') || rest.contains('?') {
                let pat = if case_sensitive {
                    rest.to_string()
                } else {
                    rest.to_lowercase()
                };
                TokenKind::Glob(pat)
            } else {
                let s = if case_sensitive {
                    rest.to_string()
                } else {
                    rest.to_lowercase()
                };
                TokenKind::Substring(s)
            };

            if is_exclude {
                exclusions.push(token);
            } else {
                group_tokens.push(token);
            }
        }

        if !group_tokens.is_empty() {
            or_groups.push(group_tokens);
        }
    }

    ParsedPattern {
        or_groups,
        exclusions,
    }
}

/// Iterative glob matcher supporting `*` (0+ chars) and `?` (1 char).
fn glob_match(pattern: &str, text: &str) -> bool {
    let pat = pattern.as_bytes();
    let txt = text.as_bytes();
    let (mut pi, mut ti) = (0, 0);
    let (mut star_pi, mut star_ti) = (usize::MAX, 0);

    while ti < txt.len() {
        if pi < pat.len() && (pat[pi] == b'?' || pat[pi] == txt[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pat.len() && pat[pi] == b'*' {
            star_pi = pi;
            star_ti = ti;
            pi += 1;
        } else if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }

    while pi < pat.len() && pat[pi] == b'*' {
        pi += 1;
    }

    pi == pat.len()
}

/// Check if a token matches a path string.
fn token_matches(token: &TokenKind, path: &str) -> bool {
    match token {
        TokenKind::Substring(s) => path.contains(s.as_str()),
        TokenKind::Glob(g) => glob_match(g, path),
        TokenKind::Exact(name) => path.rsplit("::").next() == Some(name.as_str()),
    }
}

/// Run search mode: find all leaf items matching the pattern and render them.
pub fn render_search(
    model: &CrateModel,
    pattern: &str,
    filter: &FilterArgs,
    limit: Option<&str>,
    observer_module_path: Option<&str>,
    same_crate: bool,
    reachable: Option<&ReachableInfo>,
) -> String {
    render_search_inner(
        model,
        pattern,
        filter,
        limit,
        observer_module_path,
        same_crate,
        reachable,
        None,
        None,
        false,
        None,
        None,
    )
}

/// Like `render_search`, but with optional exact-parent filtering for `--methods-of`.
/// When `methods_of` is Some, only items whose parent path segment exactly matches
/// the type name are included.
#[allow(clippy::too_many_arguments)]
pub fn render_search_methods_of(
    model: &CrateModel,
    pattern: &str,
    filter: &FilterArgs,
    limit: Option<&str>,
    observer_module_path: Option<&str>,
    same_crate: bool,
    reachable: Option<&ReachableInfo>,
    methods_of: &str,
) -> String {
    render_search_inner(
        model,
        pattern,
        filter,
        limit,
        observer_module_path,
        same_crate,
        reachable,
        Some(methods_of),
        None,
        false,
        None,
        None,
    )
}

/// Full search with all options including kind filter.
#[allow(clippy::too_many_arguments)]
pub fn render_search_filtered(
    model: &CrateModel,
    pattern: &str,
    filter: &FilterArgs,
    limit: Option<&str>,
    observer_module_path: Option<&str>,
    same_crate: bool,
    reachable: Option<&ReachableInfo>,
    methods_of: Option<&str>,
    search_kind: Option<&str>,
    members: bool,
    in_params: Option<&str>,
    in_returns: Option<&str>,
) -> String {
    render_search_inner(
        model,
        pattern,
        filter,
        limit,
        observer_module_path,
        same_crate,
        reachable,
        methods_of,
        search_kind,
        members,
        in_params,
        in_returns,
    )
}

/// Test whether an item passes the `--in-params` / `--in-returns` type filters.
///
/// Each filter is a pre-parsed `(ParsedPattern, case_sensitive)` pair. Returns `false` for
/// non-function items and for functions that fail either filter.
fn matches_type_filter(
    item: &Item,
    params_filter: Option<&(ParsedPattern, bool)>,
    returns_filter: Option<&(ParsedPattern, bool)>,
) -> bool {
    let ItemEnum::Function(f) = &item.inner else {
        return false;
    };
    if let Some((parsed, cs)) = params_filter {
        let any_param = f.sig.inputs.iter().any(|(_, ty)| {
            let s = render::format_type_pub(ty);
            let s = if *cs { s } else { s.to_lowercase() };
            parsed
                .or_groups
                .iter()
                .any(|g| g.iter().all(|tok| token_matches(tok, &s)))
        });
        if !any_param {
            return false;
        }
    }
    if let Some((parsed, cs)) = returns_filter {
        let Some(ret_ty) = &f.sig.output else {
            return false;
        };
        let s = render::format_type_pub(ret_ty);
        let s = if *cs { s } else { s.to_lowercase() };
        if !parsed
            .or_groups
            .iter()
            .any(|g| g.iter().all(|tok| token_matches(tok, &s)))
        {
            return false;
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn render_search_inner(
    model: &CrateModel,
    pattern: &str,
    filter: &FilterArgs,
    limit: Option<&str>,
    observer_module_path: Option<&str>,
    same_crate: bool,
    reachable: Option<&ReachableInfo>,
    methods_of: Option<&str>,
    search_kind: Option<&str>,
    members: bool,
    in_params: Option<&str>,
    in_returns: Option<&str>,
) -> String {
    let crate_name = model.crate_name();
    let observer = observer_module_path
        .map(|p| {
            if p.contains("::") || p == crate_name {
                p.to_string()
            } else {
                format!("{crate_name}::{p}")
            }
        })
        .unwrap_or_else(|| crate_name.to_string());

    // Collect all leaf items
    let mut leaves = Vec::new();
    if let Some(root) = model.root_module() {
        walk_module(
            model,
            root,
            "",
            filter,
            &observer,
            same_crate,
            reachable,
            &mut leaves,
        );
    }

    // Smart-case: all-lowercase → case-insensitive, any uppercase → case-sensitive
    let case_sensitive = pattern.chars().any(|c| c.is_uppercase());
    let parsed = parse_pattern(pattern, case_sensitive);

    // Positive pattern matching (OR of AND groups).
    // When no name pattern is given but a type filter is active, skip name filtering.
    let mut matched: Vec<&LeafItem> = if parsed.or_groups.is_empty()
        && (in_params.is_some() || in_returns.is_some())
    {
        leaves.iter().collect()
    } else {
        leaves
            .iter()
            .filter(|leaf| {
                let path = if case_sensitive {
                    leaf.path.clone()
                } else {
                    leaf.path.to_lowercase()
                };
                parsed
                    .or_groups
                    .iter()
                    .any(|group| group.iter().all(|tok| token_matches(tok, &path)))
            })
            .collect()
    };

    // Global exclusions
    if !parsed.exclusions.is_empty() {
        matched.retain(|leaf| {
            let path = if case_sensitive {
                leaf.path.clone()
            } else {
                leaf.path.to_lowercase()
            };
            !parsed
                .exclusions
                .iter()
                .any(|tok| token_matches(tok, &path))
        });
    }

    // Member filtering (after exclusions, before --methods-of)
    if members {
        // --members: expand matched types to include all their children
        let type_paths: HashSet<&str> = matched
            .iter()
            .filter(|l| {
                matches!(
                    l.kind,
                    LeafKind::Struct | LeafKind::Enum | LeafKind::Trait | LeafKind::Union
                )
            })
            .map(|l| l.path.as_str())
            .collect();
        let mut seen: HashSet<&str> = matched.iter().map(|l| l.path.as_str()).collect();
        let mut extra: Vec<&LeafItem> = Vec::new();
        for leaf in &leaves {
            if is_member(leaf)
                && let Some(parent) = leaf.path.rsplit_once("::").map(|(p, _)| p)
                && type_paths.contains(parent)
                && !seen.contains(leaf.path.as_str())
            {
                seen.insert(&leaf.path);
                extra.push(leaf);
            }
        }
        matched.extend(extra);
    } else if methods_of.is_none() {
        // Default: suppress members unless exact name match on a token
        matched.retain(|leaf| {
            if !is_member(leaf) {
                return true;
            }
            let name = leaf.path.rsplit("::").next().unwrap_or(&leaf.path);
            let name_cmp = if case_sensitive {
                name.to_string()
            } else {
                name.to_lowercase()
            };
            parsed
                .or_groups
                .iter()
                .flat_map(|g| g.iter())
                .any(|tok| match tok {
                    TokenKind::Substring(s) | TokenKind::Exact(s) => *s == name_cmp,
                    TokenKind::Glob(_) => false,
                })
        });

        // Inject parent types as context headers for remaining orphan members
        let member_parents: HashSet<&str> = matched
            .iter()
            .filter(|l| is_member(l))
            .filter_map(|l| l.path.rsplit_once("::").map(|(p, _)| p))
            .collect();
        let matched_paths: HashSet<&str> = matched.iter().map(|l| l.path.as_str()).collect();
        let mut extra: Vec<&LeafItem> = Vec::new();
        for leaf in &leaves {
            if matches!(
                leaf.kind,
                LeafKind::Struct | LeafKind::Enum | LeafKind::Trait | LeafKind::Union
            ) && member_parents.contains(leaf.path.as_str())
                && !matched_paths.contains(leaf.path.as_str())
            {
                extra.push(leaf);
            }
        }
        matched.extend(extra);
    }

    // --methods-of: exact parent-type segment matching
    if let Some(type_name) = methods_of {
        let suffix = format!("::{type_name}::");
        let prefix = format!("{type_name}::");
        matched.retain(|leaf| leaf.path.contains(&suffix) || leaf.path.starts_with(&prefix));
    }

    // --in-params / --in-returns: parse once, then filter by function signature type strings
    let params_filter = in_params.map(|p| {
        let cs = p.chars().any(|c| c.is_uppercase());
        (parse_pattern(p, cs), cs)
    });
    let returns_filter = in_returns.map(|p| {
        let cs = p.chars().any(|c| c.is_uppercase());
        (parse_pattern(p, cs), cs)
    });
    if params_filter.is_some() || returns_filter.is_some() {
        matched.retain(|leaf| {
            matches_type_filter(leaf.item, params_filter.as_ref(), returns_filter.as_ref())
        });
    }

    // --search-kind: include only matching kinds
    if let Some(kind_spec) = search_kind {
        let kinds: Vec<&str> = kind_spec.split(',').map(|s| s.trim()).collect();
        matched.retain(|leaf| kinds.iter().any(|k| leaf.kind.matches_kind_str(k)));
    }

    // Sort
    if members {
        // Path-based sort: groups members with their parent type
        matched.sort_by(|a, b| a.path.cmp(&b.path));
    } else {
        // Default: primary by kind, secondary by path alphabetically
        matched.sort_by(|a, b| {
            a.kind
                .sort_key()
                .cmp(&b.kind.sort_key())
                .then_with(|| a.path.cmp(&b.path))
        });
    }

    let total = matched.len();
    let (offset, search_limit) = parse_search_limit(limit);
    let offset = offset.min(total);
    let end = search_limit
        .map(|n| (offset + n).min(total))
        .unwrap_or(total);
    let skipped_before = offset;
    let skipped_after = total - end;

    // Render
    let mut output = format!("// crate {crate_name} — search: \"{pattern}\" ({total} results)\n",);

    if skipped_before > 0 {
        output.push_str(&format!("// (skipped {skipped_before} results)\n"));
    }

    let mut prev_parent: Option<&str> = None;
    for leaf in &matched[offset..end] {
        let parent = leaf.path.rsplit_once("::").map(|(p, _)| p);

        if let Some(p) = parent
            && Some(p) == prev_parent
            && is_member(leaf)
        {
            let member_name = leaf.path.rsplit_once("::").unwrap().1;
            let padding = " ".repeat(p.len().saturating_sub(1));
            render_collapsed_member(&mut output, model, leaf, &padding, member_name);
        } else {
            render_leaf(&mut output, model, leaf, filter);
        }

        prev_parent = parent;
    }

    if skipped_after > 0 {
        output.push_str(&format!("// ... and {skipped_after} more results\n"));
    }

    if total == 0 {
        let word_count = pattern.split_whitespace().count();
        if word_count >= 4 {
            output.push_str(&format!(
                "// hint: search uses AND matching — all {word_count} words must appear in one item's path. Try fewer words.\n"
            ));
        }
    }

    output
}

/// Walk a module recursively, collecting all leaf items.
#[allow(clippy::too_many_arguments)]
fn walk_module<'a>(
    model: &'a CrateModel,
    module_item: &'a Item,
    parent_path: &str,
    args: &FilterArgs,
    observer: &str,
    same_crate: bool,
    reachable: Option<&ReachableInfo>,
    leaves: &mut Vec<LeafItem<'a>>,
) {
    let children = model.module_children(module_item);

    for (child_id, child) in &children {
        // Visibility check
        if let Some(info) = reachable {
            if !info.reachable.contains(child_id) {
                continue;
            }
        } else if !matches!(child.visibility, Visibility::Default)
            && !is_visible_from(model, child, child_id, observer, same_crate)
        {
            continue;
        }

        let name = child
            .name
            .as_deref()
            .or(if let ItemEnum::Use(u) = &child.inner {
                Some(u.name.as_str())
            } else {
                None
            });
        let Some(name) = name else { continue };

        let child_path = if parent_path.is_empty() {
            name.to_string()
        } else {
            format!("{parent_path}::{name}")
        };

        match &child.inner {
            ItemEnum::Module(_) => {
                // Flatten path for glob-private modules: items get parent's path
                let walk_path =
                    if reachable.is_some_and(|info| info.glob_private_modules.contains(child_id)) {
                        parent_path.to_string()
                    } else {
                        child_path.clone()
                    };
                walk_module(
                    model, child, &walk_path, args, observer, same_crate, reachable, leaves,
                );
            }
            ItemEnum::Struct(s) => {
                if !args.no_structs {
                    // Struct itself is a leaf (name match)
                    leaves.push(LeafItem {
                        path: child_path.clone(),
                        item: child,
                        kind: LeafKind::Struct,
                        context: LeafContext::None,
                    });
                }
                // Named struct fields are always walked (needed for --methods-of)
                walk_struct_fields(model, s, &child_path, observer, same_crate, leaves);
            }
            ItemEnum::Enum(e) => {
                if !args.no_enums {
                    leaves.push(LeafItem {
                        path: child_path.clone(),
                        item: child,
                        kind: LeafKind::Enum,
                        context: LeafContext::None,
                    });
                }
                // Enum variants are always walked (needed for --methods-of)
                for variant_id in &e.variants {
                    if let Some(variant_item) = model.krate.index.get(variant_id) {
                        let vname = variant_item.name.as_deref().unwrap_or("?");
                        leaves.push(LeafItem {
                            path: format!("{child_path}::{vname}"),
                            item: variant_item,
                            kind: LeafKind::Variant,
                            context: LeafContext::Variant,
                        });
                    }
                }
            }
            ItemEnum::Trait(t) => {
                if !args.no_traits {
                    leaves.push(LeafItem {
                        path: child_path.clone(),
                        item: child,
                        kind: LeafKind::Trait,
                        context: LeafContext::None,
                    });
                }
                // Trait items are always walked (needed for --methods-of)
                for item_id in &t.items {
                    if let Some(trait_item) = model.krate.index.get(item_id) {
                        let iname = trait_item.name.as_deref().unwrap_or("?");
                        let item_path = format!("{child_path}::{iname}");
                        match &trait_item.inner {
                            ItemEnum::Function(_) if !args.no_functions => {
                                leaves.push(LeafItem {
                                    path: item_path,
                                    item: trait_item,
                                    kind: LeafKind::Function,
                                    context: LeafContext::ImplMethod,
                                });
                            }
                            ItemEnum::AssocType { .. } if !args.no_aliases => {
                                leaves.push(LeafItem {
                                    path: item_path,
                                    item: trait_item,
                                    kind: LeafKind::AssocType,
                                    context: LeafContext::AssocType,
                                });
                            }
                            ItemEnum::AssocConst { .. } if !args.no_constants => {
                                leaves.push(LeafItem {
                                    path: item_path,
                                    item: trait_item,
                                    kind: LeafKind::AssocConst,
                                    context: LeafContext::AssocConst,
                                });
                            }
                            _ => {}
                        }
                    }
                }
            }
            ItemEnum::Function(_) if !args.no_functions => {
                leaves.push(LeafItem {
                    path: child_path,
                    item: child,
                    kind: LeafKind::Function,
                    context: LeafContext::None,
                });
            }
            ItemEnum::TypeAlias(_) if !args.no_aliases => {
                leaves.push(LeafItem {
                    path: child_path,
                    item: child,
                    kind: LeafKind::TypeAlias,
                    context: LeafContext::None,
                });
            }
            ItemEnum::Constant { .. } if !args.no_constants => {
                leaves.push(LeafItem {
                    path: child_path,
                    item: child,
                    kind: LeafKind::Constant,
                    context: LeafContext::None,
                });
            }
            ItemEnum::Static(_) if !args.no_constants => {
                leaves.push(LeafItem {
                    path: child_path,
                    item: child,
                    kind: LeafKind::Static,
                    context: LeafContext::None,
                });
            }
            ItemEnum::Union(u) => {
                if !args.no_unions {
                    leaves.push(LeafItem {
                        path: child_path.clone(),
                        item: child,
                        kind: LeafKind::Union,
                        context: LeafContext::None,
                    });
                }
                // Union fields are always walked (needed for --methods-of)
                for field_id in &u.fields {
                    if let Some(field_item) = model.krate.index.get(field_id) {
                        if !is_visible_from(model, field_item, field_id, observer, same_crate)
                            && !matches!(field_item.visibility, Visibility::Public)
                        {
                            continue;
                        }
                        if let ItemEnum::StructField(ty) = &field_item.inner {
                            let fname = field_item.name.as_deref().unwrap_or("?");
                            leaves.push(LeafItem {
                                path: format!("{child_path}::{fname}"),
                                item: field_item,
                                kind: LeafKind::Field,
                                context: LeafContext::Field { field_type: ty },
                            });
                        }
                    }
                }
            }
            ItemEnum::Macro(_) if !args.no_macros => {
                leaves.push(LeafItem {
                    path: child_path,
                    item: child,
                    kind: LeafKind::Macro,
                    context: LeafContext::None,
                });
            }
            ItemEnum::ProcMacro(pm) if !args.no_macros => {
                let kind = match pm.kind {
                    MacroKind::Bang => LeafKind::ProcMacro,
                    MacroKind::Attr => LeafKind::ProcAttrMacro,
                    MacroKind::Derive => LeafKind::ProcDeriveMacro,
                };
                leaves.push(LeafItem {
                    path: child_path,
                    item: child,
                    kind,
                    context: LeafContext::None,
                });
            }
            ItemEnum::Use(use_item) if !use_item.is_glob => {
                // Follow the re-export target to check --no-* filters
                let target = use_item
                    .id
                    .as_ref()
                    .and_then(|id| model.krate.index.get(id));
                let skip = target
                    .map(|t| match &t.inner {
                        ItemEnum::Function(_) => args.no_functions,
                        ItemEnum::Struct(_) => args.no_structs,
                        ItemEnum::Enum(_) => args.no_enums,
                        ItemEnum::Trait(_) => args.no_traits,
                        ItemEnum::Union(_) => args.no_unions,
                        ItemEnum::Constant { .. } => args.no_constants,
                        ItemEnum::Static(_) => args.no_constants,
                        ItemEnum::TypeAlias(_) => args.no_aliases,
                        ItemEnum::Macro(_) => args.no_macros,
                        ItemEnum::ProcMacro(_) => args.no_macros,
                        _ => false,
                    })
                    .unwrap_or(false);
                if !skip {
                    let source = &use_item.source;
                    leaves.push(LeafItem {
                        path: child_path,
                        item: child,
                        kind: LeafKind::Use,
                        context: LeafContext::Use {
                            source: source.clone(),
                        },
                    });
                }
            }
            _ => {}
        }
    }

    // Walk impl blocks for types in this module
    walk_impl_blocks(
        model,
        module_item,
        parent_path,
        args,
        observer,
        same_crate,
        reachable,
        leaves,
    );
}

/// Walk named struct fields as leaf items.
fn walk_struct_fields<'a>(
    model: &'a CrateModel,
    s: &Struct,
    struct_path: &str,
    observer: &str,
    same_crate: bool,
    leaves: &mut Vec<LeafItem<'a>>,
) {
    if let StructKind::Plain { fields, .. } = &s.kind {
        for field_id in fields {
            if let Some(field_item) = model.krate.index.get(field_id) {
                if !is_visible_from(model, field_item, field_id, observer, same_crate)
                    && !matches!(field_item.visibility, Visibility::Public)
                {
                    continue;
                }
                if let ItemEnum::StructField(ty) = &field_item.inner {
                    let fname = field_item.name.as_deref().unwrap_or("?");
                    leaves.push(LeafItem {
                        path: format!("{struct_path}::{fname}"),
                        item: field_item,
                        kind: LeafKind::Field,
                        context: LeafContext::Field { field_type: ty },
                    });
                }
            }
        }
    }
}

/// Walk public struct fields (cross-crate only — no visibility check beyond `pub`).
fn walk_struct_fields_pub<'a>(
    model: &'a CrateModel,
    s: &Struct,
    struct_path: &str,
    leaves: &mut Vec<LeafItem<'a>>,
) {
    if let StructKind::Plain { fields, .. } = &s.kind {
        for field_id in fields {
            if let Some(field_item) = model.krate.index.get(field_id)
                && let ItemEnum::StructField(ty) = &field_item.inner
                && matches!(field_item.visibility, Visibility::Public)
            {
                let fname = field_item.name.as_deref().unwrap_or("?");
                leaves.push(LeafItem {
                    path: format!("{struct_path}::{fname}"),
                    item: field_item,
                    kind: LeafKind::Field,
                    context: LeafContext::Field { field_type: ty },
                });
            }
        }
    }
}

/// Walk impl blocks for types defined in this module.
#[allow(clippy::too_many_arguments)]
fn walk_impl_blocks<'a>(
    model: &'a CrateModel,
    module_item: &'a Item,
    parent_path: &str,
    args: &FilterArgs,
    observer: &str,
    same_crate: bool,
    reachable: Option<&ReachableInfo>,
    leaves: &mut Vec<LeafItem<'a>>,
) {
    let children = model.module_children(module_item);
    let mut impl_ids: Vec<Id> = Vec::new();

    for (child_id, child) in &children {
        if let Some(info) = reachable {
            if !info.reachable.contains(child_id) {
                continue;
            }
        } else if !matches!(child.visibility, Visibility::Default)
            && !is_visible_from(model, child, child_id, observer, same_crate)
        {
            continue;
        }
        let impls = match &child.inner {
            ItemEnum::Struct(s) => &s.impls,
            ItemEnum::Enum(e) => &e.impls,
            ItemEnum::Union(u) => &u.impls,
            _ => continue,
        };
        impl_ids.extend(impls.iter().cloned());
    }

    for impl_id in &impl_ids {
        let Some(impl_item) = model.krate.index.get(impl_id) else {
            continue;
        };
        let ItemEnum::Impl(impl_block) = &impl_item.inner else {
            continue;
        };

        if !args.all && (impl_block.is_synthetic || impl_block.blanket_impl.is_some()) {
            continue;
        }

        let type_name = render::format_type_pub(&impl_block.for_);
        if type_name.is_empty() {
            continue;
        }

        let is_trait_impl = impl_block.trait_.is_some();

        // Build the prefix: parent_path::TypeName
        let type_path = if parent_path.is_empty() {
            type_name.clone()
        } else {
            format!("{parent_path}::{type_name}")
        };

        for item_id in &impl_block.items {
            let Some(item) = model.krate.index.get(item_id) else {
                continue;
            };

            // Visibility check for inherent impl items
            if !is_trait_impl
                && !matches!(item.visibility, Visibility::Default | Visibility::Public)
                && !is_visible_from(model, item, item_id, observer, same_crate)
            {
                continue;
            }

            let iname = item.name.as_deref().unwrap_or("?");
            let item_path = format!("{type_path}::{iname}");

            match &item.inner {
                ItemEnum::Function(_) if !args.no_functions => {
                    leaves.push(LeafItem {
                        path: item_path,
                        item,
                        kind: LeafKind::Function,
                        context: LeafContext::ImplMethod,
                    });
                }
                ItemEnum::AssocType { .. } if !args.no_aliases => {
                    leaves.push(LeafItem {
                        path: item_path,
                        item,
                        kind: LeafKind::AssocType,
                        context: LeafContext::AssocType,
                    });
                }
                ItemEnum::AssocConst { .. } if !args.no_constants => {
                    leaves.push(LeafItem {
                        path: item_path,
                        item,
                        kind: LeafKind::AssocConst,
                        context: LeafContext::AssocConst,
                    });
                }
                _ => {}
            }
        }
    }
}

// === Rendering ===

/// Render a single leaf item as a one-liner.
pub(crate) fn render_leaf(
    output: &mut String,
    model: &CrateModel,
    leaf: &LeafItem,
    args: &FilterArgs,
) {
    // Doc comment: first line only (suppressed by --no-docs / --compact)
    if !args.no_docs
        && !args.compact
        && let Some(docs) = &leaf.item.docs
        && let Some(first_line) = docs.lines().next()
        && !first_line.is_empty()
    {
        output.push_str(&format!("/// {first_line}\n"));
    }

    // Attribute markers for search results (deprecated / non_exhaustive only)
    if leaf.item.deprecation.is_some() {
        output.push_str("[deprecated] ");
    }
    if leaf
        .item
        .attrs
        .iter()
        .any(|a| matches!(a, Attribute::NonExhaustive))
    {
        output.push_str("[non_exhaustive] ");
    }

    match leaf.kind {
        LeafKind::Function => render_function_leaf(output, leaf),
        LeafKind::Struct => render_struct_leaf(output, model, leaf, args),
        LeafKind::Enum => {
            let summary = collect_impl_summary(model, leaf.item, args);
            if summary.is_empty() {
                output.push_str(&format!("enum {};\n", leaf.path));
            } else {
                output.push_str(&format!("enum {}; {summary}\n", leaf.path));
            }
        }
        LeafKind::Trait => {
            output.push_str(&format!("trait {};\n", leaf.path));
        }
        LeafKind::Union => {
            let summary = collect_impl_summary(model, leaf.item, args);
            if summary.is_empty() {
                output.push_str(&format!("union {};\n", leaf.path));
            } else {
                output.push_str(&format!("union {}; {summary}\n", leaf.path));
            }
        }
        LeafKind::Field => render_field_leaf(output, leaf),
        LeafKind::Variant => render_variant_leaf(output, model, leaf),
        LeafKind::Constant => render_constant_leaf(output, leaf),
        LeafKind::Static => render_static_leaf(output, leaf),
        LeafKind::TypeAlias => render_type_alias_leaf(output, leaf),
        LeafKind::Macro => {
            output.push_str(&format!("macro {}!;\n", leaf.path));
        }
        LeafKind::ProcMacro => {
            output.push_str(&format!("proc_macro {}!;\n", leaf.path));
        }
        LeafKind::ProcAttrMacro => {
            output.push_str(&format!("attr_macro #[{}];\n", leaf.path));
        }
        LeafKind::ProcDeriveMacro => {
            output.push_str(&format!("derive_macro #[derive({})];\n", leaf.path));
        }
        LeafKind::AssocType => render_assoc_type_leaf(output, leaf),
        LeafKind::AssocConst => render_assoc_const_leaf(output, leaf),
        LeafKind::Use => render_use_leaf(output, leaf),
    }
}

/// Render a member item with collapsed `-::member` continuation format.
fn render_collapsed_member(
    output: &mut String,
    model: &CrateModel,
    leaf: &LeafItem,
    padding: &str,
    member_name: &str,
) {
    match (&leaf.kind, &leaf.context) {
        (LeafKind::Field, LeafContext::Field { field_type }) => {
            output.push_str(&format!(
                "{padding}-::{member_name}: {};\n",
                render::format_type_pub(field_type)
            ));
        }
        (LeafKind::Function, _) => {
            if let ItemEnum::Function(f) = &leaf.item.inner {
                let sig = render::format_function_sig_pub(member_name, f, "");
                // Strip the name prefix to get just params+return
                let params_ret = sig.find('(').map(|i| &sig[i..]).unwrap_or("()");
                output.push_str(&format!("{padding}-::{member_name}{params_ret};\n"));
            }
        }
        (LeafKind::Variant, _) => {
            render_collapsed_variant(output, model, leaf, padding, member_name);
        }
        (LeafKind::AssocType, _) => {
            if let ItemEnum::AssocType {
                type_: Some(ty), ..
            } = &leaf.item.inner
            {
                output.push_str(&format!(
                    "{padding}-::type {member_name} = {};\n",
                    render::format_type_pub(ty)
                ));
            } else {
                output.push_str(&format!("{padding}-::type {member_name};\n"));
            }
        }
        (LeafKind::AssocConst, _) => {
            if let ItemEnum::AssocConst { type_, value } = &leaf.item.inner {
                let val = value.as_deref().unwrap_or("..");
                output.push_str(&format!(
                    "{padding}-::const {member_name}: {} = {val};\n",
                    render::format_type_pub(type_)
                ));
            }
        }
        _ => {
            // Non-member kinds don't reach collapsed rendering
        }
    }
}

/// Render an enum variant in collapsed format.
fn render_collapsed_variant(
    output: &mut String,
    model: &CrateModel,
    leaf: &LeafItem,
    padding: &str,
    member_name: &str,
) {
    let ItemEnum::Variant(variant) = &leaf.item.inner else {
        return;
    };
    match &variant.kind {
        VariantKind::Plain => {
            output.push_str(&format!("{padding}-::{member_name},\n"));
        }
        VariantKind::Tuple(fields) => {
            let field_strs: Vec<String> = fields
                .iter()
                .map(|f_id| {
                    f_id.as_ref()
                        .and_then(|id| model.krate.index.get(id))
                        .map(|f| {
                            if let ItemEnum::StructField(ty) = &f.inner {
                                render::format_type_pub(ty)
                            } else {
                                "?".to_string()
                            }
                        })
                        .unwrap_or_else(|| "_".to_string())
                })
                .collect();
            output.push_str(&format!(
                "{padding}-::{member_name}({}),\n",
                field_strs.join(", ")
            ));
        }
        VariantKind::Struct { fields, .. } => {
            let field_strs: Vec<String> = fields
                .iter()
                .filter_map(|fid| model.krate.index.get(fid))
                .filter_map(|f| {
                    if let ItemEnum::StructField(ty) = &f.inner {
                        let fname = f.name.as_deref().unwrap_or("?");
                        Some(format!("{fname}: {}", render::format_type_pub(ty)))
                    } else {
                        None
                    }
                })
                .collect();
            output.push_str(&format!(
                "{padding}-::{member_name} {{ {} }},\n",
                field_strs.join(", ")
            ));
        }
    }
}

fn render_function_leaf(output: &mut String, leaf: &LeafItem) {
    if let ItemEnum::Function(f) = &leaf.item.inner {
        let sig = render::format_function_sig_pub(
            &leaf.path, f, "", // no visibility prefix in search results
        );
        let wc = render::format_where_clause_compact_pub(&f.generics);
        output.push_str(&format!("{sig}{wc};\n"));
    }
}

fn render_struct_leaf(output: &mut String, model: &CrateModel, leaf: &LeafItem, args: &FilterArgs) {
    let ItemEnum::Struct(s) = &leaf.item.inner else {
        return;
    };
    let generics = render::format_generics_pub(&s.generics);
    let wc = render::format_where_clause_compact_pub(&s.generics);
    let summary = collect_impl_summary(model, leaf.item, args);
    let suffix = if summary.is_empty() {
        String::new()
    } else {
        format!(" {summary}")
    };

    match &s.kind {
        StructKind::Unit => {
            output.push_str(&format!("struct {}{}{wc};\n", leaf.path, generics));
        }
        StructKind::Tuple(fields) => {
            let field_strs: Vec<String> = fields
                .iter()
                .map(|f_id| {
                    f_id.as_ref()
                        .and_then(|id| model.krate.index.get(id))
                        .map(|f| {
                            if let ItemEnum::StructField(ty) = &f.inner {
                                render::format_type_pub(ty)
                            } else {
                                "?".to_string()
                            }
                        })
                        .unwrap_or_else(|| "_".to_string())
                })
                .collect();
            output.push_str(&format!(
                "struct {}{}({}){wc};{suffix}\n",
                leaf.path,
                generics,
                field_strs.join(", ")
            ));
        }
        StructKind::Plain { .. } => {
            output.push_str(&format!(
                "struct {}{}{wc} {{ .. }};{suffix}\n",
                leaf.path, generics
            ));
        }
    }
}

fn render_field_leaf(output: &mut String, leaf: &LeafItem) {
    if let LeafContext::Field { field_type } = &leaf.context {
        output.push_str(&format!(
            "field {}: {};\n",
            leaf.path,
            render::format_type_pub(field_type)
        ));
    }
}

fn render_variant_leaf(output: &mut String, model: &CrateModel, leaf: &LeafItem) {
    let ItemEnum::Variant(variant) = &leaf.item.inner else {
        return;
    };
    match &variant.kind {
        VariantKind::Plain => {
            output.push_str(&format!("variant {};\n", leaf.path));
        }
        VariantKind::Tuple(fields) => {
            let field_strs: Vec<String> = fields
                .iter()
                .map(|f_id| {
                    f_id.as_ref()
                        .and_then(|id| model.krate.index.get(id))
                        .map(|f| {
                            if let ItemEnum::StructField(ty) = &f.inner {
                                render::format_type_pub(ty)
                            } else {
                                "?".to_string()
                            }
                        })
                        .unwrap_or_else(|| "_".to_string())
                })
                .collect();
            output.push_str(&format!(
                "variant {}({});\n",
                leaf.path,
                field_strs.join(", ")
            ));
        }
        VariantKind::Struct { fields, .. } => {
            let field_strs: Vec<String> = fields
                .iter()
                .filter_map(|fid| model.krate.index.get(fid))
                .filter_map(|f| {
                    if let ItemEnum::StructField(ty) = &f.inner {
                        let fname = f.name.as_deref().unwrap_or("?");
                        Some(format!("{fname}: {}", render::format_type_pub(ty)))
                    } else {
                        None
                    }
                })
                .collect();
            output.push_str(&format!(
                "variant {} {{ {} }};\n",
                leaf.path,
                field_strs.join(", ")
            ));
        }
    }
}

fn render_constant_leaf(output: &mut String, leaf: &LeafItem) {
    if let ItemEnum::Constant { type_, const_: c } = &leaf.item.inner {
        let val = c.value.as_deref().unwrap_or("..");
        output.push_str(&format!(
            "const {}: {} = {val};\n",
            leaf.path,
            render::format_type_pub(type_)
        ));
    }
}

fn render_static_leaf(output: &mut String, leaf: &LeafItem) {
    if let ItemEnum::Static(s) = &leaf.item.inner {
        let mutability = if s.is_mutable { "mut " } else { "" };
        output.push_str(&format!(
            "static {mutability}{}: {};\n",
            leaf.path,
            render::format_type_pub(&s.type_)
        ));
    }
}

fn render_type_alias_leaf(output: &mut String, leaf: &LeafItem) {
    if let ItemEnum::TypeAlias(ta) = &leaf.item.inner {
        let generics = render::format_generics_pub(&ta.generics);
        let wc = render::format_where_clause_compact_pub(&ta.generics);
        output.push_str(&format!(
            "type {}{generics}{wc} = {};\n",
            leaf.path,
            render::format_type_pub(&ta.type_)
        ));
    }
}

fn render_assoc_type_leaf(output: &mut String, leaf: &LeafItem) {
    if let ItemEnum::AssocType { type_, .. } = &leaf.item.inner {
        if let Some(ty) = type_ {
            output.push_str(&format!(
                "type {} = {};\n",
                leaf.path,
                render::format_type_pub(ty)
            ));
        } else {
            output.push_str(&format!("type {};\n", leaf.path));
        }
    }
}

fn render_assoc_const_leaf(output: &mut String, leaf: &LeafItem) {
    if let ItemEnum::AssocConst { type_, value } = &leaf.item.inner {
        let val = value.as_deref().unwrap_or("..");
        output.push_str(&format!(
            "const {}: {} = {val};\n",
            leaf.path,
            render::format_type_pub(type_)
        ));
    }
}

fn render_use_leaf(output: &mut String, leaf: &LeafItem) {
    if let LeafContext::Use { source } = &leaf.context {
        let name = leaf.path.rsplit("::").next().unwrap_or(&leaf.path);
        // Check if it's an alias (name differs from source's last segment)
        let source_name = source.rsplit("::").next().unwrap_or(source);
        if name == source_name {
            output.push_str(&format!("use {source};\n"));
        } else {
            output.push_str(&format!("use {source} as {name};\n"));
        }
    }
}

/// Build an inline `// impl (N methods), impl Trait1, impl Trait2` summary for a type.
fn collect_impl_summary(model: &CrateModel, item: &Item, args: &FilterArgs) -> String {
    let impls = match &item.inner {
        ItemEnum::Struct(s) => &s.impls,
        ItemEnum::Enum(e) => &e.impls,
        ItemEnum::Union(u) => &u.impls,
        _ => return String::new(),
    };

    let mut parts: Vec<String> = Vec::new();

    for impl_id in impls {
        let Some(impl_item) = model.krate.index.get(impl_id) else {
            continue;
        };
        let ItemEnum::Impl(impl_block) = &impl_item.inner else {
            continue;
        };

        if !args.all && (impl_block.is_synthetic || impl_block.blanket_impl.is_some()) {
            continue;
        }

        if let Some(trait_) = &impl_block.trait_ {
            parts.push(format!("impl {}", render::format_path_pub(trait_)));
        } else {
            // Inherent impl: count visible methods
            let method_count = impl_block
                .items
                .iter()
                .filter(|id| {
                    model
                        .krate
                        .index
                        .get(id)
                        .map(|i| matches!(i.inner, ItemEnum::Function(_)))
                        .unwrap_or(false)
                })
                .count();
            if method_count > 0 {
                parts.push(format!("impl ({method_count} methods)"));
            }
        }
    }

    if parts.is_empty() {
        return String::new();
    }

    if parts.len() > 5 {
        let extra = parts.len() - 5;
        parts.truncate(5);
        parts.push(format!("+ {extra} more"));
    }

    format!("// {}", parts.join(", "))
}

// === Cross-crate index search ===

use crate::cross_crate::{AccessibleItemKind, CrossCrateIndex};

/// Search a `CrossCrateIndex` and render results with accessible paths.
///
/// Iterates accessible items, matches against the pattern, and renders
/// each match as a one-liner using the accessible path instead of the
/// internal crate path.
#[allow(clippy::too_many_arguments)]
pub fn search_cross_crate_index(
    index: &CrossCrateIndex,
    _facade_crate_name: &str,
    pattern: &str,
    filter: &FilterArgs,
    limit: Option<&str>,
    search_kind: Option<&str>,
    methods_of: Option<&str>,
    members: bool,
    in_params: Option<&str>,
    in_returns: Option<&str>,
) -> String {
    // Smart-case
    let case_sensitive = pattern.chars().any(|c| c.is_uppercase());
    let parsed = parse_pattern(pattern, case_sensitive);

    // Collect matching leaves from the index, paired with crate_idx for O(1) model lookup
    let mut matched: Vec<(usize, LeafItem)> = Vec::new();

    for entry in &index.items {
        if entry.item_kind == AccessibleItemKind::Module {
            continue;
        }
        if should_skip_kind(entry.item_kind, filter) {
            continue;
        }

        let model = &index.source_models[entry.crate_idx].0;
        let Some(item) = model.krate.index.get(&entry.item_id) else {
            continue;
        };

        let kind = match entry.item_kind {
            AccessibleItemKind::Function => LeafKind::Function,
            AccessibleItemKind::Struct => LeafKind::Struct,
            AccessibleItemKind::Enum => LeafKind::Enum,
            AccessibleItemKind::Trait => LeafKind::Trait,
            AccessibleItemKind::Union => LeafKind::Union,
            AccessibleItemKind::TypeAlias => LeafKind::TypeAlias,
            AccessibleItemKind::Constant => LeafKind::Constant,
            AccessibleItemKind::Static => LeafKind::Static,
            AccessibleItemKind::Macro => LeafKind::Macro,
            AccessibleItemKind::ProcMacro => LeafKind::ProcMacro,
            AccessibleItemKind::ProcAttrMacro => LeafKind::ProcAttrMacro,
            AccessibleItemKind::ProcDeriveMacro => LeafKind::ProcDeriveMacro,
            AccessibleItemKind::Module => continue,
        };

        let mut type_leaves = Vec::new();
        if matches!(
            entry.item_kind,
            AccessibleItemKind::Struct | AccessibleItemKind::Enum | AccessibleItemKind::Union
        ) {
            walk_type_impl_items(
                model,
                item,
                &entry.accessible_path,
                filter,
                &mut type_leaves,
            );
        }
        if entry.item_kind == AccessibleItemKind::Trait {
            walk_trait_items(
                model,
                item,
                &entry.accessible_path,
                filter,
                &mut type_leaves,
            );
        }
        // Walk struct fields for cross-crate types
        if entry.item_kind == AccessibleItemKind::Struct
            && let ItemEnum::Struct(s) = &item.inner
        {
            walk_struct_fields_pub(model, s, &entry.accessible_path, &mut type_leaves);
        }
        // Walk enum variants for cross-crate types
        if entry.item_kind == AccessibleItemKind::Enum
            && let ItemEnum::Enum(e) = &item.inner
        {
            for variant_id in &e.variants {
                if let Some(v) = model.krate.index.get(variant_id) {
                    let vname = v.name.as_deref().unwrap_or("?");
                    type_leaves.push(LeafItem {
                        path: format!("{}::{vname}", entry.accessible_path),
                        item: v,
                        kind: LeafKind::Variant,
                        context: LeafContext::Variant,
                    });
                }
            }
        }
        // Walk union fields for cross-crate types
        if entry.item_kind == AccessibleItemKind::Union
            && let ItemEnum::Union(u) = &item.inner
        {
            for field_id in &u.fields {
                if let Some(f) = model.krate.index.get(field_id)
                    && let ItemEnum::StructField(ty) = &f.inner
                    && matches!(f.visibility, Visibility::Public)
                {
                    let fname = f.name.as_deref().unwrap_or("?");
                    type_leaves.push(LeafItem {
                        path: format!("{}::{fname}", entry.accessible_path),
                        item: f,
                        kind: LeafKind::Field,
                        context: LeafContext::Field { field_type: ty },
                    });
                }
            }
        }

        matched.push((
            entry.crate_idx,
            LeafItem {
                path: entry.accessible_path.clone(),
                item,
                kind,
                context: LeafContext::None,
            },
        ));

        for tl in type_leaves {
            matched.push((entry.crate_idx, tl));
        }
    }

    // Pattern matching. When no name pattern is given but a type filter is active, skip name filtering.
    let mut filtered: Vec<(usize, &LeafItem)> = if parsed.or_groups.is_empty()
        && (in_params.is_some() || in_returns.is_some())
    {
        matched.iter().map(|(ci, leaf)| (*ci, leaf)).collect()
    } else {
        matched
            .iter()
            .filter(|(_, leaf)| {
                let path = if case_sensitive {
                    leaf.path.clone()
                } else {
                    leaf.path.to_lowercase()
                };
                parsed
                    .or_groups
                    .iter()
                    .any(|group| group.iter().all(|tok| token_matches(tok, &path)))
            })
            .map(|(ci, leaf)| (*ci, leaf))
            .collect()
    };

    // Global exclusions
    if !parsed.exclusions.is_empty() {
        filtered.retain(|(_, leaf)| {
            let path = if case_sensitive {
                leaf.path.clone()
            } else {
                leaf.path.to_lowercase()
            };
            !parsed
                .exclusions
                .iter()
                .any(|tok| token_matches(tok, &path))
        });
    }

    // Member filtering (mirrors local-crate logic)
    if members {
        let type_paths: HashSet<&str> = filtered
            .iter()
            .filter(|(_, l)| {
                matches!(
                    l.kind,
                    LeafKind::Struct | LeafKind::Enum | LeafKind::Trait | LeafKind::Union
                )
            })
            .map(|(_, l)| l.path.as_str())
            .collect();
        let mut seen: HashSet<&str> = filtered.iter().map(|(_, l)| l.path.as_str()).collect();
        let mut extra: Vec<(usize, &LeafItem)> = Vec::new();
        for (ci, leaf) in &matched {
            if is_member(leaf)
                && let Some(parent) = leaf.path.rsplit_once("::").map(|(p, _)| p)
                && type_paths.contains(parent)
                && !seen.contains(leaf.path.as_str())
            {
                seen.insert(&leaf.path);
                extra.push((*ci, leaf));
            }
        }
        filtered.extend(extra);
    } else if methods_of.is_none() {
        filtered.retain(|(_, leaf)| {
            if !is_member(leaf) {
                return true;
            }
            let name = leaf.path.rsplit("::").next().unwrap_or(&leaf.path);
            let name_cmp = if case_sensitive {
                name.to_string()
            } else {
                name.to_lowercase()
            };
            parsed
                .or_groups
                .iter()
                .flat_map(|g| g.iter())
                .any(|tok| match tok {
                    TokenKind::Substring(s) | TokenKind::Exact(s) => *s == name_cmp,
                    TokenKind::Glob(_) => false,
                })
        });

        // Inject parent types as context headers
        let member_parents: HashSet<&str> = filtered
            .iter()
            .filter(|(_, l)| is_member(l))
            .filter_map(|(_, l)| l.path.rsplit_once("::").map(|(p, _)| p))
            .collect();
        let matched_paths: HashSet<&str> = filtered.iter().map(|(_, l)| l.path.as_str()).collect();
        let mut extra: Vec<(usize, &LeafItem)> = Vec::new();
        for (ci, leaf) in &matched {
            if matches!(
                leaf.kind,
                LeafKind::Struct | LeafKind::Enum | LeafKind::Trait | LeafKind::Union
            ) && member_parents.contains(leaf.path.as_str())
                && !matched_paths.contains(leaf.path.as_str())
            {
                extra.push((*ci, leaf));
            }
        }
        filtered.extend(extra);
    }

    // --methods-of
    if let Some(type_name) = methods_of {
        let suffix = format!("::{type_name}::");
        let prefix_pat = format!("{type_name}::");
        filtered
            .retain(|(_, leaf)| leaf.path.contains(&suffix) || leaf.path.starts_with(&prefix_pat));
    }

    // --in-params / --in-returns: parse once, then filter by function signature type strings
    let params_filter = in_params.map(|p| {
        let cs = p.chars().any(|c| c.is_uppercase());
        (parse_pattern(p, cs), cs)
    });
    let returns_filter = in_returns.map(|p| {
        let cs = p.chars().any(|c| c.is_uppercase());
        (parse_pattern(p, cs), cs)
    });
    if params_filter.is_some() || returns_filter.is_some() {
        filtered.retain(|(_, leaf)| {
            matches_type_filter(leaf.item, params_filter.as_ref(), returns_filter.as_ref())
        });
    }

    // --search-kind
    if let Some(kind_spec) = search_kind {
        let kinds: Vec<&str> = kind_spec.split(',').map(|s| s.trim()).collect();
        filtered.retain(|(_, leaf)| kinds.iter().any(|k| leaf.kind.matches_kind_str(k)));
    }

    // Sort
    if members {
        filtered.sort_by(|a, b| a.1.path.cmp(&b.1.path));
    } else {
        filtered.sort_by(|a, b| {
            a.1.kind
                .sort_key()
                .cmp(&b.1.kind.sort_key())
                .then_with(|| a.1.path.cmp(&b.1.path))
        });
    }

    let total = filtered.len();
    let (offset, search_limit) = parse_search_limit(limit);
    let offset = offset.min(total);
    let end = search_limit
        .map(|n| (offset + n).min(total))
        .unwrap_or(total);
    let skipped_after = total - end;

    // Render with collapsed display
    let mut output = String::new();
    let mut prev_parent: Option<&str> = None;

    for &(ci, leaf) in &filtered[offset..end] {
        let model = &index.source_models[ci].0;
        let parent = leaf.path.rsplit_once("::").map(|(p, _)| p);

        if let Some(p) = parent
            && Some(p) == prev_parent
            && is_member(leaf)
        {
            let member_name = leaf.path.rsplit_once("::").unwrap().1;
            let padding = " ".repeat(p.len().saturating_sub(1));
            render_collapsed_member(&mut output, model, leaf, &padding, member_name);
        } else {
            render_leaf(&mut output, model, leaf, filter);
        }

        prev_parent = parent;
    }

    if skipped_after > 0 {
        output.push_str(&format!("// ... and {skipped_after} more results\n"));
    }

    output
}

/// Check if a kind should be skipped based on filter flags.
fn should_skip_kind(kind: AccessibleItemKind, filter: &FilterArgs) -> bool {
    match kind {
        AccessibleItemKind::Function => filter.no_functions,
        AccessibleItemKind::Struct => filter.no_structs,
        AccessibleItemKind::Enum => filter.no_enums,
        AccessibleItemKind::Trait => filter.no_traits,
        AccessibleItemKind::Union => filter.no_unions,
        AccessibleItemKind::TypeAlias => filter.no_aliases,
        AccessibleItemKind::Constant | AccessibleItemKind::Static => filter.no_constants,
        AccessibleItemKind::Macro
        | AccessibleItemKind::ProcMacro
        | AccessibleItemKind::ProcAttrMacro
        | AccessibleItemKind::ProcDeriveMacro => filter.no_macros,
        AccessibleItemKind::Module => false,
    }
}

/// Walk impl blocks of a type, collecting method/assoc items with accessible paths.
fn walk_type_impl_items<'a>(
    model: &'a CrateModel,
    type_item: &'a Item,
    type_path: &str,
    filter: &FilterArgs,
    leaves: &mut Vec<LeafItem<'a>>,
) {
    let impls = match &type_item.inner {
        ItemEnum::Struct(s) => &s.impls,
        ItemEnum::Enum(e) => &e.impls,
        ItemEnum::Union(u) => &u.impls,
        _ => return,
    };

    for impl_id in impls {
        let Some(impl_item) = model.krate.index.get(impl_id) else {
            continue;
        };
        let ItemEnum::Impl(impl_block) = &impl_item.inner else {
            continue;
        };

        if !filter.all && (impl_block.is_synthetic || impl_block.blanket_impl.is_some()) {
            continue;
        }

        for item_id in &impl_block.items {
            let Some(item) = model.krate.index.get(item_id) else {
                continue;
            };
            let iname = item.name.as_deref().unwrap_or("?");
            let item_path = format!("{type_path}::{iname}");

            match &item.inner {
                ItemEnum::Function(_) if !filter.no_functions => {
                    leaves.push(LeafItem {
                        path: item_path,
                        item,
                        kind: LeafKind::Function,
                        context: LeafContext::ImplMethod,
                    });
                }
                ItemEnum::AssocType { .. } if !filter.no_aliases => {
                    leaves.push(LeafItem {
                        path: item_path,
                        item,
                        kind: LeafKind::AssocType,
                        context: LeafContext::AssocType,
                    });
                }
                ItemEnum::AssocConst { .. } if !filter.no_constants => {
                    leaves.push(LeafItem {
                        path: item_path,
                        item,
                        kind: LeafKind::AssocConst,
                        context: LeafContext::AssocConst,
                    });
                }
                _ => {}
            }
        }
    }
}

/// Walk trait items, collecting methods/assoc items with accessible paths.
fn walk_trait_items<'a>(
    model: &'a CrateModel,
    trait_item: &'a Item,
    trait_path: &str,
    filter: &FilterArgs,
    leaves: &mut Vec<LeafItem<'a>>,
) {
    let ItemEnum::Trait(t) = &trait_item.inner else {
        return;
    };
    for item_id in &t.items {
        let Some(item) = model.krate.index.get(item_id) else {
            continue;
        };
        let iname = item.name.as_deref().unwrap_or("?");
        let item_path = format!("{trait_path}::{iname}");

        match &item.inner {
            ItemEnum::Function(_) if !filter.no_functions => {
                leaves.push(LeafItem {
                    path: item_path,
                    item,
                    kind: LeafKind::Function,
                    context: LeafContext::ImplMethod,
                });
            }
            ItemEnum::AssocType { .. } if !filter.no_aliases => {
                leaves.push(LeafItem {
                    path: item_path,
                    item,
                    kind: LeafKind::AssocType,
                    context: LeafContext::AssocType,
                });
            }
            ItemEnum::AssocConst { .. } if !filter.no_constants => {
                leaves.push(LeafItem {
                    path: item_path,
                    item,
                    kind: LeafKind::AssocConst,
                    context: LeafContext::AssocConst,
                });
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_match_star_any() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*", ""));
    }

    #[test]
    fn glob_match_question_mark() {
        assert!(glob_match("?", "a"));
        assert!(!glob_match("?", ""));
        assert!(!glob_match("?", "ab"));
    }

    #[test]
    fn glob_match_prefix_star() {
        assert!(glob_match("*Enum", "PlainEnum"));
        assert!(glob_match("*Enum", "Enum"));
        assert!(!glob_match("*Enum", "EnumX"));
    }

    #[test]
    fn glob_match_suffix_star() {
        assert!(glob_match("a*b", "ab"));
        assert!(glob_match("a*b", "axb"));
        assert!(glob_match("a*b", "axxxb"));
        assert!(!glob_match("a*b", "axbc"));
    }

    #[test]
    fn glob_match_mid_question() {
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
        assert!(!glob_match("a?c", "abbc"));
    }

    #[test]
    fn glob_match_multi_star() {
        assert!(glob_match("*x*y*", "xxy"));
        assert!(glob_match("*x*y*", "aaxbbycc"));
        assert!(!glob_match("*x*y*", "aaxbb"));
    }

    #[test]
    fn glob_match_exact() {
        assert!(glob_match("hello", "hello"));
        assert!(!glob_match("hello", "hello!"));
        assert!(!glob_match("hello", "hell"));
    }

    #[test]
    fn glob_match_empty() {
        assert!(glob_match("", ""));
        assert!(!glob_match("", "a"));
    }

    #[test]
    fn parse_pattern_basic() {
        let p = parse_pattern("foo bar", true);
        assert_eq!(p.or_groups.len(), 1);
        assert_eq!(p.or_groups[0].len(), 2);
        assert!(p.exclusions.is_empty());
    }

    #[test]
    fn parse_pattern_or_groups() {
        let p = parse_pattern("foo,bar baz", true);
        assert_eq!(p.or_groups.len(), 2);
        assert_eq!(p.or_groups[0].len(), 1);
        assert_eq!(p.or_groups[1].len(), 2);
    }

    #[test]
    fn parse_pattern_exclusion() {
        let p = parse_pattern("foo -bar", true);
        assert_eq!(p.or_groups.len(), 1);
        assert_eq!(p.exclusions.len(), 1);
    }

    #[test]
    fn parse_pattern_exact() {
        let p = parse_pattern("=Name", true);
        assert_eq!(p.or_groups.len(), 1);
        assert!(matches!(&p.or_groups[0][0], TokenKind::Exact(n) if n == "Name"));
    }

    #[test]
    fn parse_pattern_exclude_exact() {
        let p = parse_pattern("-=Name", true);
        assert!(p.or_groups.is_empty());
        assert_eq!(p.exclusions.len(), 1);
        assert!(matches!(&p.exclusions[0], TokenKind::Exact(n) if n == "Name"));
    }

    #[test]
    fn parse_pattern_glob_detected() {
        let p = parse_pattern("foo*bar", true);
        assert!(matches!(&p.or_groups[0][0], TokenKind::Glob(_)));
    }

    #[test]
    fn parse_pattern_case_insensitive() {
        let p = parse_pattern("Foo", false);
        assert!(matches!(&p.or_groups[0][0], TokenKind::Substring(s) if s == "foo"));
    }
}
