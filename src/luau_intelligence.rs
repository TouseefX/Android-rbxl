//! Lightweight project-wide Luau intelligence for the built-in mobile editor.
//!
//! This indexes Script instances in the local DataModel, resolves instance and
//! modern string requires (`./`, `../`, and `@self`), and exposes ModuleScript
//! members without requiring a filesystem or an external language server.

use crate::{rbxl, schema};
use rbx_dom_weak::{types::Ref, WeakDom};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub label: String,
    pub detail: String,
    /// Text inserted when accepted and characters replaced before the caret.
    pub insert_text: String,
    pub replace_chars: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDiagnostic {
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub referent: Ref,
    pub line: usize,
    pub member: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub referent: Ref,
    pub line: usize,
    pub preview: String,
}

#[derive(Debug, Clone)]
pub struct RenameEdit {
    pub referent: Ref,
    pub source: String,
    pub replacements: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSymbol {
    pub name: String,
    pub detail: String,
    pub path: String,
    pub referent: Ref,
    pub line: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectIndex {
    /// Normalized DataModel path or unambiguous module name -> members.
    modules: HashMap<String, BTreeMap<String, String>>,
    module_refs: HashMap<String, Ref>,
    module_sources: HashMap<Ref, String>,
    script_sources: HashMap<Ref, String>,
    /// Every script's slash-separated virtual path in the DataModel.
    paths: HashMap<Ref, String>,
}

impl ProjectIndex {
    /// Cheap change token used by the UI cache. It hashes script identity,
    /// hierarchy names, committed source, and unsaved tab overrides without
    /// rebuilding module exports/dependency graphs every frame.
    pub fn fingerprint<'a>(
        dom: &WeakDom,
        overrides: impl IntoIterator<Item = (Ref, &'a str)>,
    ) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let overrides: HashMap<Ref, &str> = overrides.into_iter().collect();
        fn walk(
            dom: &WeakDom,
            referent: Ref,
            overrides: &HashMap<Ref, &str>,
            hasher: &mut impl Hasher,
        ) {
            let Some(instance) = dom.get_by_ref(referent) else { return };
            referent.hash(hasher);
            instance.name.hash(hasher);
            instance.class.as_str().hash(hasher);
            if matches!(instance.class.as_str(), "Script" | "LocalScript" | "ModuleScript") {
                if let Some(source) = overrides.get(&referent) {
                    source.hash(hasher);
                } else {
                    rbxl::get_source(dom, referent).unwrap_or_default().hash(hasher);
                }
            }
            for &child in instance.children() {
                walk(dom, child, overrides, hasher);
            }
        }
        walk(dom, dom.root_ref(), &overrides, &mut hasher);
        hasher.finish()
    }

    pub fn build(dom: &WeakDom) -> Self {
        Self::build_with_overrides(dom, std::iter::empty::<(Ref, &str)>())
    }

    /// Build from the DataModel while preferring unsaved editor buffers.
    pub fn build_with_overrides<'a>(
        dom: &WeakDom,
        overrides: impl IntoIterator<Item = (Ref, &'a str)>,
    ) -> Self {
        let mut index = Self::default();
        index.walk(dom, dom.root_ref(), &mut Vec::new());
        for (referent, source) in overrides {
            index.script_sources.insert(referent, source.to_string());
            if index.module_sources.contains_key(&referent) {
                index.module_sources.insert(referent, source.to_string());
                let members = exported_members(source);
                let keys: Vec<String> = index.module_refs.iter()
                    .filter_map(|(key, value)| (*value == referent).then_some(key.clone()))
                    .collect();
                for key in keys {
                    index.modules.insert(key, members.clone());
                }
            }
        }
        index
    }

    fn walk(&mut self, dom: &WeakDom, referent: Ref, path: &mut Vec<String>) {
        let Some(instance) = dom.get_by_ref(referent) else { return };
        let is_root = referent == dom.root_ref();
        if !is_root {
            path.push(instance.name.clone());
            self.paths.insert(referent, path.join("/"));
        }

        let is_script = matches!(
            instance.class.as_str(),
            "Script" | "LocalScript" | "ModuleScript"
        );
        let source = is_script.then(|| rbxl::get_source(dom, referent).unwrap_or_default());
        if let Some(source) = &source {
            self.script_sources.insert(referent, source.clone());
        }

        if instance.class.as_str() == "ModuleScript" {
            let source = source.unwrap_or_default();
            let members = exported_members(&source);
            let full_key = normalize_path(&path.join("/"));
            let name_key = normalize_path(&instance.name);
            self.modules.insert(full_key.clone(), members.clone());
            self.modules.insert(name_key.clone(), members);
            self.module_refs.insert(full_key, referent);
            self.module_refs.entry(name_key).or_insert(referent);
            self.module_sources.insert(referent, source);
        }

        for &child in instance.children() {
            self.walk(dom, child, path);
        }
        if !is_root {
            path.pop();
        }
    }

    /// Parse the workspace symbol catalog once. UI filtering should reuse this
    /// list rather than reparsing every large script on every rendered frame.
    pub fn all_workspace_symbols(&self) -> Vec<WorkspaceSymbol> {
        let mut result = Vec::new();
        for (&referent, source) in &self.script_sources {
            let path = self.paths.get(&referent).cloned().unwrap_or_default();
            for (name, detail, line) in source_symbols(source) {
                result.push(WorkspaceSymbol { name, detail, path: path.clone(), referent, line });
            }
        }
        result.sort_by_key(|item| (item.name.to_ascii_lowercase(), item.path.to_ascii_lowercase()));
        result
    }

    pub fn filter_workspace_symbols(catalog: &[WorkspaceSymbol], query: &str) -> Vec<WorkspaceSymbol> {
        let query = query.trim().to_ascii_lowercase();
        let mut result: Vec<_> = catalog.iter().filter(|item| {
            query.is_empty() || fuzzy_match(
                &format!("{} {} {}", item.name, item.detail, item.path).to_ascii_lowercase(),
                &query,
            )
        }).cloned().collect();
        result.sort_by_key(|item| {
            let name = item.name.to_ascii_lowercase();
            let rank = if query.is_empty() { 2 } else if name == query { 0 } else if name.starts_with(&query) { 1 } else { 2 };
            (rank, name, item.path.to_ascii_lowercase())
        });
        result.truncate(100);
        result
    }

    /// Complete `Alias.partial` at a character cursor position, resolving Alias
    /// from a require declaration anywhere in the file.
    pub fn complete_at(
        &self,
        current_script: Ref,
        source: &str,
        cursor_char: usize,
    ) -> Vec<Completion> {
        let cursor_byte = char_to_byte(source, cursor_char);
        if let Some(typed) = require_string_at_cursor(&source[..cursor_byte]) {
            return self.complete_require_path(current_script, typed);
        }
        if let Some((alias, prefix)) = member_expression_at_end(&source[..cursor_byte]) {
            let aliases = require_aliases(source);
            if let Some(request) = aliases.get(alias) {
                let resolved = self.resolve_request(current_script, request);
                let key = normalize_path(&resolved);
                let members = self.modules.get(&key).or_else(|| {
                    key.rsplit('/').next().and_then(|name| self.modules.get(name))
                });
                if let Some(members) = members {
                    return members
                        .iter()
                        .filter(|(member, _)| member.to_ascii_lowercase().starts_with(&prefix.to_ascii_lowercase()))
                        .take(12)
                        .map(|(member, signature)| Completion {
                            label: member.clone(),
                            detail: format!("{signature}  ·  {request} → {resolved}"),
                            insert_text: member.clone(),
                            replace_chars: prefix.chars().count(),
                        })
                        .collect();
                }
            }
            // A dotted expression is a member lookup, never a new lexical
            // keyword. Falling through used to turn `game.f` into `game.game`.
            return roblox_member_completions(alias, prefix);
        }
        let before_cursor = &source[..cursor_byte];
        let mut completions = lexical_completions(before_cursor);
        let prefix = identifier_fragment(before_cursor);
        if prefix.len() >= 2 {
            let lower = prefix.to_ascii_lowercase();
            let mut seen: std::collections::HashSet<String> =
                completions.iter().map(|item| item.label.to_ascii_lowercase()).collect();
            for (&referent, path) in &self.paths {
                if !self.module_sources.contains_key(&referent) { continue; }
                let name = path.rsplit('/').next().unwrap_or(path);
                let candidate = name.to_ascii_lowercase();
                if (candidate.starts_with(&lower) || (lower.len() >= 3 && common_prefix_len(&candidate, &lower) >= 2))
                    && seen.insert(candidate)
                {
                    completions.push(Completion {
                        label: name.to_string(),
                        detail: format!("ModuleScript · {path}"),
                        insert_text: name.to_string(),
                        replace_chars: prefix.chars().count(),
                    });
                }
            }
        }
        completions.truncate(12);
        completions
    }

    fn complete_require_path(&self, current_script: Ref, typed: &str) -> Vec<Completion> {
        let current = self.paths.get(&current_script).map(String::as_str).unwrap_or("");
        let parent = current.rsplit_once('/').map_or("", |(path, _)| path);
        let mut candidates = std::collections::BTreeSet::new();

        if typed.is_empty() {
            return ["./", "../", "@self/"]
                .into_iter()
                .map(|prefix| Completion {
                    label: prefix.into(),
                    detail: "Luau require path prefix".into(),
                    insert_text: prefix.into(),
                    replace_chars: 0,
                })
                .collect();
        }
        for (&referent, path) in &self.paths {
            if !self.module_sources.contains_key(&referent) || referent == current_script {
                continue;
            }
            let candidate = if typed.starts_with("@self/") {
                path.strip_prefix(&format!("{current}/"))
                    .map(|rest| format!("@self/{rest}"))
            } else if typed.starts_with('@') {
                Some(format!("@{path}"))
            } else {
                Some(relative_module_path(parent, path))
            };
            if let Some(candidate) = candidate {
                if candidate.to_ascii_lowercase().starts_with(&typed.to_ascii_lowercase()) {
                    candidates.insert(candidate);
                }
            }
        }

        candidates.into_iter().take(12).map(|path| Completion {
            label: path.clone(),
            detail: "ModuleScript path".into(),
            insert_text: path,
            replace_chars: typed.chars().count(),
        }).collect()
    }

    /// Resolve the module member under the caret to its defining ModuleScript
    /// and source line.
    pub fn definition_at(
        &self,
        current_script: Ref,
        source: &str,
        cursor_char: usize,
    ) -> Option<Definition> {
        let expression = member_expression_at_cursor(source, cursor_char)?;
        let (alias, member) = match expression.rsplit_once('.') {
            Some((alias, member)) if is_identifier(alias) && is_identifier(member) => {
                (alias, Some(member))
            }
            None if is_identifier(expression) => (expression, None),
            _ => return None,
        };
        let request = require_aliases(source).get(alias)?.clone();
        let target = self.resolve_module_ref(current_script, &request)?;
        let module_source = self.module_sources.get(&target)?;
        Some(Definition {
            referent: target,
            line: member.and_then(|name| member_definition_line(module_source, name)).unwrap_or(1),
            member: member.map(str::to_string),
        })
    }

    /// Find project-wide uses of a resolved module or one of its exported
    /// members. Require aliases are resolved independently in every script.
    pub fn references(&self, definition: &Definition) -> Vec<Reference> {
        let mut result = Vec::new();
        for (&script_ref, source) in &self.script_sources {
            let aliases = require_aliases(source);
            for (alias, request) in aliases {
                if self.resolve_module_ref(script_ref, &request) != Some(definition.referent) {
                    continue;
                }
                let needle = definition.member.as_ref()
                    .map_or_else(|| alias.to_string(), |member| format!("{alias}.{member}"));
                for (line_index, line) in source.lines().enumerate() {
                    let code = line.split("--").next().unwrap_or("");
                    if contains_identifier(code, &needle) {
                        result.push(Reference {
                            referent: script_ref,
                            line: line_index + 1,
                            preview: line.trim().to_string(),
                        });
                    }
                }
            }
        }
        result.sort_by(|a, b| {
            self.paths.get(&a.referent).cmp(&self.paths.get(&b.referent))
                .then(a.line.cmp(&b.line))
        });
        result.dedup_by(|a, b| a.referent == b.referent && a.line == b.line);
        result
    }

    /// Produce non-overlapping source updates for a resolved exported member.
    /// Module aliases themselves are intentionally not renamed project-wide,
    /// because each consumer owns its local alias.
    pub fn rename_member(&self, definition: &Definition, new_name: &str) -> Vec<RenameEdit> {
        let Some(old_name) = definition.member.as_deref() else { return Vec::new() };
        if !is_identifier(new_name) || old_name == new_name { return Vec::new() }
        let mut edits = Vec::new();
        for (&script_ref, original) in &self.script_sources {
            let mut source = original.clone();
            let mut replacements = 0;

            // Consumers can use different aliases for the same target module.
            for (alias, request) in require_aliases(original) {
                if self.resolve_module_ref(script_ref, &request) == Some(definition.referent) {
                    let (updated, count) = replace_identifier_token(
                        &source,
                        &format!("{alias}.{old_name}"),
                        &format!("{alias}.{new_name}"),
                    );
                    source = updated;
                    replacements += count;
                }
            }

            if script_ref == definition.referent {
                // Rename only owners that actually define this export; a broad
                // `.Old` replacement could corrupt unrelated objects in the
                // same module.
                for owner in module_member_owners(original, old_name) {
                    let old = format!("{owner}.{old_name}");
                    let new = format!("{owner}.{new_name}");
                    let (updated, count) = replace_identifier_token(&source, &old, &new);
                    source = updated;
                    replacements += count;
                }
                for (old, new) in [
                    (format!("export type {old_name}"), format!("export type {new_name}")),
                    (format!("export function {old_name}"), format!("export function {new_name}")),
                ] {
                    let (updated, count) = replace_identifier_token(&source, &old, &new);
                    source = updated;
                    replacements += count;
                }
                // Returned table fields don't have a leading dot.
                for line in original.lines() {
                    let trimmed = line.trim_start();
                    if trimmed.starts_with(&format!("{old_name} =")) {
                        let offset = line.len() - trimmed.len();
                        let needle = &line[..offset + old_name.len()];
                        let replacement = format!("{}{}", &line[..offset], new_name);
                        source = source.replacen(needle, &replacement, 1);
                        replacements += 1;
                    }
                }
            }
            if replacements > 0 {
                edits.push(RenameEdit { referent: script_ref, source, replacements });
            }
        }
        edits
    }

    /// Validate string/DataModel requires and member accesses against the local
    /// project index. These are editor warnings, separate from Luau grammar
    /// errors produced by the compiler.
    pub fn diagnostics(&self, current_script: Ref, source: &str) -> Vec<ProjectDiagnostic> {
        let aliases = require_aliases_with_lines(source);
        let mut diagnostics = Vec::new();
        for (alias, request, require_line) in aliases {
            let resolved = self.resolve_request(current_script, &request);
            let key = normalize_path(&resolved);
            let members = self.modules.get(&key).or_else(|| {
                key.rsplit('/').next().and_then(|name| self.modules.get(name))
            });
            let Some(members) = members else {
                diagnostics.push(ProjectDiagnostic {
                    line: require_line,
                    message: format!("Unresolved module '{request}' (looked for '{resolved}')"),
                });
                continue;
            };

            let needle = format!("{alias}.");
            for (line_index, line) in source.lines().enumerate() {
                let code = line.split("--").next().unwrap_or("");
                let mut remainder = code;
                while let Some(position) = remainder.find(&needle) {
                    let after = &remainder[position + needle.len()..];
                    let member = identifier_start(after);
                    if is_identifier(member) && !members.contains_key(member) {
                        diagnostics.push(ProjectDiagnostic {
                            line: line_index + 1,
                            message: format!("Unknown member '{member}' on module '{alias}'"),
                        });
                    }
                    remainder = &after[member.len()..];
                }
            }
        }
        diagnostics.extend(self.cycle_diagnostics(current_script, source));
        diagnostics.sort_by_key(|diagnostic| diagnostic.line);
        diagnostics.dedup();
        diagnostics
    }

    fn cycle_diagnostics(&self, current_script: Ref, current_source: &str) -> Vec<ProjectDiagnostic> {
        if !self.module_sources.contains_key(&current_script) {
            return Vec::new();
        }
        let graph = self.dependency_graph(Some((current_script, current_source)));
        let mut path = Vec::new();
        let mut visiting = std::collections::HashSet::new();
        if let Some(cycle) = find_immediate_cycle(current_script, current_script, &graph, &mut visiting, &mut path) {
            let names: Vec<String> = cycle.iter().map(|referent| {
                self.paths.get(referent).and_then(|path| path.rsplit('/').next())
                    .unwrap_or("Module").to_string()
            }).collect();
            let line = graph.get(&current_script)
                .and_then(|edges| edges.iter().find(|edge| !edge.deferred).map(|edge| edge.line))
                .unwrap_or(1);
            return vec![ProjectDiagnostic {
                line,
                message: format!("Immediate cyclic module dependency: {}", names.join(" → ")),
            }];
        }

        // Deferred edges are not initialization errors by themselves, but flag
        // a reciprocal path so users know calling that function too early can
        // recreate the cycle.
        let mut result = Vec::new();
        if let Some(edges) = graph.get(&current_script) {
            for edge in edges.iter().filter(|edge| edge.deferred) {
                if has_path(edge.target, current_script, &graph, &mut std::collections::HashSet::new()) {
                    result.push(ProjectDiagnostic {
                        line: edge.line,
                        message: "Deferred cyclic require inside a function; safe only after both modules finish initialization".into(),
                    });
                }
            }
        }
        result
    }

    fn dependency_graph(&self, source_override: Option<(Ref, &str)>) -> HashMap<Ref, Vec<DependencyEdge>> {
        let mut graph = HashMap::new();
        for (&referent, stored_source) in &self.module_sources {
            let source = source_override
                .filter(|(override_ref, _)| *override_ref == referent)
                .map_or(stored_source.as_str(), |(_, source)| source);
            let mut edges = Vec::new();
            for require in scan_requires(source) {
                let resolved = self.resolve_request(referent, &require.request);
                let key = normalize_path(&resolved);
                let target = self.module_refs.get(&key).copied().or_else(|| {
                    key.rsplit('/').next().and_then(|name| self.module_refs.get(name).copied())
                });
                if let Some(target) = target {
                    edges.push(DependencyEdge { target, line: require.line, deferred: require.deferred });
                }
            }
            graph.insert(referent, edges);
        }
        graph
    }

    fn resolve_module_ref(&self, current_script: Ref, request: &str) -> Option<Ref> {
        let key = normalize_path(&self.resolve_request(current_script, request));
        self.module_refs.get(&key).copied().or_else(|| {
            key.rsplit('/').next().and_then(|name| self.module_refs.get(name).copied())
        })
    }

    fn resolve_request(&self, current_script: Ref, request: &str) -> String {
        let current = self.paths.get(&current_script).cloned().unwrap_or_default();
        if request == "@self" {
            return current;
        }
        if let Some(rest) = request.strip_prefix("@self/") {
            return collapse_path(&format!("{current}/{rest}"));
        }
        if request.starts_with("./") || request.starts_with("../") {
            let parent = current.rsplit_once('/').map_or("", |(p, _)| p);
            return collapse_path(&format!("{parent}/{request}"));
        }
        // Aliases emitted in the exported .luaurc map the filesystem project
        // back onto the DataModel hierarchy.
        let request_lower = request.to_ascii_lowercase();
        if request_lower.starts_with("@src/") {
            return collapse_path(&request[5..]);
        }
        if request_lower.starts_with("@shared/") {
            return collapse_path(&format!("ReplicatedStorage/{}", &request[8..]));
        }
        // User aliases normally come from .luaurc. Until project folders land,
        // map @Service/foo naturally onto a top-level DataModel service.
        if let Some(rest) = request.strip_prefix('@') {
            return collapse_path(rest);
        }
        collapse_path(request)
    }
}

/// Replace the identifier fragment immediately before the caret while
/// preserving everything after it. Cursor positions use egui's character
/// indexing rather than UTF-8 byte offsets.
#[derive(Debug, Clone)]
struct DependencyEdge {
    target: Ref,
    line: usize,
    deferred: bool,
}

#[derive(Debug)]
struct RequireUse {
    request: String,
    line: usize,
    deferred: bool,
}

fn scan_requires(source: &str) -> Vec<RequireUse> {
    let mut result = Vec::new();
    let mut blocks: Vec<bool> = Vec::new(); // true means function scope
    for (line_index, raw) in source.lines().enumerate() {
        let code = raw.split("--").next().unwrap_or("").trim();
        let closes = code == "end" || code.starts_with("end;") || code.starts_with("until ");
        if closes { blocks.pop(); }

        let in_function = blocks.iter().any(|is_function| *is_function);
        let mut remainder = code;
        while let Some(start) = remainder.find("require(") {
            let after = &remainder[start + "require(".len()..];
            if let Some(end) = after.rfind(')') {
                let expression = after[..end].trim();
                result.push(RequireUse {
                    request: require_path(expression),
                    line: line_index + 1,
                    deferred: in_function,
                });
                remainder = &after[end + 1..];
            } else {
                break;
            }
        }

        let function = code.starts_with("function ")
            || code.starts_with("local function ")
            || code.starts_with("const function ")
            || (code.contains("= function") && !code.contains(" end"));
        let other_block = (code.starts_with("if ") && code.ends_with("then"))
            || ((code.starts_with("for ") || code.starts_with("while ")) && code.ends_with("do"))
            || code == "do" || code == "repeat";
        if function && !code.ends_with("end") {
            blocks.push(true);
        } else if other_block {
            blocks.push(false);
        }
    }
    result
}

fn find_immediate_cycle(
    origin: Ref,
    node: Ref,
    graph: &HashMap<Ref, Vec<DependencyEdge>>,
    visiting: &mut std::collections::HashSet<Ref>,
    path: &mut Vec<Ref>,
) -> Option<Vec<Ref>> {
    visiting.insert(node);
    path.push(node);
    for edge in graph.get(&node).into_iter().flatten().filter(|edge| !edge.deferred) {
        if edge.target == origin {
            let mut cycle = path.clone();
            cycle.push(origin);
            return Some(cycle);
        }
        if !visiting.contains(&edge.target) {
            if let Some(cycle) = find_immediate_cycle(origin, edge.target, graph, visiting, path) {
                return Some(cycle);
            }
        }
    }
    path.pop();
    visiting.remove(&node);
    None
}

fn has_path(
    node: Ref,
    target: Ref,
    graph: &HashMap<Ref, Vec<DependencyEdge>>,
    visited: &mut std::collections::HashSet<Ref>,
) -> bool {
    if node == target { return true }
    if !visited.insert(node) { return false }
    graph.get(&node).into_iter().flatten().any(|edge| {
        has_path(edge.target, target, graph, visited)
    })
}

/// Roblox-engine-aware checks layered on top of the Luau parser. Reflection is
/// used only where it is authoritative; methods/events are not guessed.
pub fn semantic_diagnostics(source: &str) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = Vec::new();
    let mut instance_vars: HashMap<String, String> = HashMap::new();
    let mut const_bindings: HashMap<String, usize> = HashMap::new();

    for (line_index, raw) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let code = raw.split("--").next().unwrap_or("").trim();

        if let Some(rest) = code.strip_prefix("const ") {
            let declaration = rest.strip_prefix("function ").unwrap_or(rest);
            let name = identifier_start(declaration);
            if is_identifier(name) {
                const_bindings.insert(name.to_string(), line_number);
            }
        } else {
            for (name, declared_line) in &const_bindings {
                let rest = code.strip_prefix(name.as_str()).unwrap_or("").trim_start();
                if rest.starts_with('=') || rest.starts_with("+=") || rest.starts_with("-=")
                    || rest.starts_with("*=") || rest.starts_with("/=") || rest.starts_with("..=")
                {
                    diagnostics.push(ProjectDiagnostic {
                        line: line_number,
                        message: format!("Cannot reassign const '{name}' declared on line {declared_line}"),
                    });
                }
            }
        }

        // Catch obvious annotated-literal mismatches without pretending to be
        // a full flow-sensitive type checker.
        if let Some((left, rhs)) = code.split_once('=') {
            if let Some((_, annotation)) = left.split_once(':') {
                let expected = annotation.trim();
                let rhs = rhs.trim();
                let mismatch = (expected == "number" && is_string_literal(rhs))
                    || (expected == "string" && (rhs.parse::<f64>().is_ok() || matches!(rhs, "true" | "false")))
                    || (expected == "boolean" && !matches!(rhs, "true" | "false") && is_literal(rhs));
                if mismatch {
                    diagnostics.push(ProjectDiagnostic {
                        line: line_number,
                        message: format!("Literal does not match annotated type '{expected}'"),
                    });
                }
            }
        }

        for class_name in quoted_call_arguments(code, "Instance.new(") {
            if !schema::class_exists(class_name) {
                diagnostics.push(ProjectDiagnostic {
                    line: line_number,
                    message: format!("Unknown Roblox class '{class_name}' in Instance.new"),
                });
            } else if !schema::class_is_creatable(class_name) {
                diagnostics.push(ProjectDiagnostic {
                    line: line_number,
                    message: format!("Roblox class '{class_name}' cannot be created with Instance.new"),
                });
            }
        }

        for service in quoted_call_arguments(code, "GetService(") {
            if !schema::class_exists(service) || !schema::class_is_service(service) {
                diagnostics.push(ProjectDiagnostic {
                    line: line_number,
                    message: format!("Unknown Roblox service '{service}'"),
                });
            }
        }

        // Infer straightforward local bindings from Instance.new so property
        // assignments can be checked against inherited reflection properties.
        if let Some(declaration) = code.strip_prefix("local ").or_else(|| code.strip_prefix("const ")) {
            if let Some((name, rhs)) = declaration.split_once('=') {
                let name = name.trim();
                if is_identifier(name) {
                    if let Some(class_name) = quoted_call_arguments(rhs, "Instance.new(").next() {
                        if schema::class_exists(class_name) {
                            instance_vars.insert(name.to_string(), class_name.to_string());
                        }
                    }
                }
            }
        }

        if let Some((lhs, rhs)) = code.split_once('=') {
            let lhs = lhs.trim();
            if let Some((variable, property)) = lhs.split_once('.') {
                if is_identifier(variable) && is_identifier(property) {
                    if let Some(class_name) = instance_vars.get(variable) {
                        match schema::resolve_property_type(class_name, property) {
                            None => diagnostics.push(ProjectDiagnostic {
                                line: line_number,
                                message: format!("Unknown property '{property}' on Roblox {class_name}"),
                            }),
                            Some(data_type) => {
                                let expected = format!("{data_type:?}");
                                let rhs = rhs.trim();
                                let mismatch = (expected.contains("Bool") && matches!(rhs, "true" | "false") == false && is_literal(rhs))
                                    || (expected.contains("String") && !is_string_literal(rhs) && is_literal(rhs));
                                if mismatch {
                                    diagnostics.push(ProjectDiagnostic {
                                        line: line_number,
                                        message: format!("Value for {class_name}.{property} does not match {expected}"),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        for token in code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.')) {
            let Some(rest) = token.strip_prefix("Enum.") else { continue };
            let mut parts = rest.split('.');
            let Some(enum_name) = parts.next() else { continue };
            let Some(item_name) = parts.next() else { continue };
            match schema::enum_item_exists(enum_name, item_name) {
                None => diagnostics.push(ProjectDiagnostic {
                    line: line_number,
                    message: format!("Unknown Roblox enum 'Enum.{enum_name}'"),
                }),
                Some(false) => diagnostics.push(ProjectDiagnostic {
                    line: line_number,
                    message: format!("Unknown item '{item_name}' on Enum.{enum_name}"),
                }),
                Some(true) => {}
            }
        }
    }
    diagnostics.sort_by_key(|diagnostic| diagnostic.line);
    diagnostics.dedup();
    diagnostics
}

fn quoted_call_arguments<'a>(line: &'a str, call: &str) -> impl Iterator<Item = &'a str> {
    line.match_indices(call).filter_map(move |(position, _)| {
        let rest = line[position + call.len()..].trim_start();
        let quote = rest.chars().next()?;
        if !matches!(quote, '"' | '\'') { return None }
        let value = &rest[quote.len_utf8()..];
        let end = value.find(quote)?;
        Some(&value[..end])
    })
}

fn is_string_literal(value: &str) -> bool {
    (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''))
        || (value.starts_with('`') && value.ends_with('`'))
}

fn is_literal(value: &str) -> bool {
    is_string_literal(value)
        || matches!(value, "true" | "false" | "nil")
        || value.parse::<f64>().is_ok()
}

pub fn apply_suggestion_at(source: &mut String, cursor_char: usize, completion: &Completion) -> usize {
    let cursor_byte = char_to_byte(source, cursor_char);
    let start_char = cursor_char.saturating_sub(completion.replace_chars);
    let start_byte = char_to_byte(source, start_char);
    source.replace_range(start_byte..cursor_byte, &completion.insert_text);
    start_char + completion.insert_text.chars().count()
}

pub fn apply_completion_at(source: &mut String, cursor_char: usize, member: &str) -> usize {
    let cursor_byte = char_to_byte(source, cursor_char);
    let prefix_bytes = source[..cursor_byte]
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .map(char::len_utf8)
        .sum::<usize>();
    let start_byte = cursor_byte - prefix_bytes;
    let start_char = source[..start_byte].chars().count();
    source.replace_range(start_byte..cursor_byte, member);
    start_char + member.chars().count()
}

fn char_to_byte(source: &str, char_index: usize) -> usize {
    source
        .char_indices()
        .nth(char_index)
        .map_or(source.len(), |(byte, _)| byte)
}

fn collapse_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let mut parts: Vec<&str> = Vec::new();
    for part in normalized.split('/').filter(|p| !p.is_empty() && *p != ".") {
        if part == ".." { parts.pop(); } else { parts.push(part); }
    }
    parts.join("/")
}

fn normalize_path(path: &str) -> String {
    let mut value = path.trim().trim_matches(['"', '\'', ' ']).replace('.', "/");
    for prefix in ["game/", "Game/"] {
        if value.starts_with(prefix) { value = value[prefix.len()..].to_string(); }
    }
    collapse_path(&value.replace(":GetService(\"", "/").replace("\")", ""))
        .to_ascii_lowercase()
}

fn parse_require_declaration(line: &str) -> Option<(&str, String)> {
    let code = line.split("--").next().unwrap_or("").trim();
    let code = code.strip_prefix("local ").or_else(|| code.strip_prefix("const "))?;
    let (alias, rhs) = code.split_once('=')?;
    let alias = alias.trim();
    if !is_identifier(alias) { return None }
    let inner = rhs.trim().strip_prefix("require(")?.strip_suffix(')')?;
    Some((alias, require_path(inner.trim())))
}

fn require_aliases(source: &str) -> HashMap<&str, String> {
    source.lines().filter_map(parse_require_declaration).collect()
}

fn require_aliases_with_lines(source: &str) -> Vec<(String, String, usize)> {
    source.lines().enumerate().filter_map(|(line, text)| {
        parse_require_declaration(text)
            .map(|(alias, request)| (alias.to_string(), request, line + 1))
    }).collect()
}

fn require_path(expression: &str) -> String {
    let quoted = expression.trim_matches(['"', '\'']);
    if quoted != expression { return quoted.to_string(); }
    expression.replace(":WaitForChild(\"", "/")
        .replace(":FindFirstChild(\"", "/")
        .replace("\")", "").replace('.', "/")
}

fn roblox_member_completions(owner: &str, prefix: &str) -> Vec<Completion> {
    let items: &[(&str, &str)] = match owner {
        "game" | "Game" => &[
            ("GetService", "Roblox DataModel service lookup"),
            ("FindService", "Find a loaded Roblox service"),
            ("IsLoaded", "Whether the place finished loading"),
            ("GetDescendants", "All descendants of the DataModel"),
            ("GetChildren", "Children of the DataModel"),
            ("Workspace", "Workspace service"),
            ("Players", "Players service"),
            ("ReplicatedStorage", "ReplicatedStorage service"),
        ],
        "workspace" | "Workspace" => &[
            ("CurrentCamera", "Current workspace camera"),
            ("FindFirstChild", "Find a child instance"),
            ("WaitForChild", "Wait for a child instance"),
            ("GetChildren", "Children of Workspace"),
            ("GetDescendants", "All Workspace descendants"),
            ("Raycast", "Cast a ray through Workspace"),
        ],
        "script" => &[
            ("Parent", "Parent instance"),
            ("Name", "Instance name"),
            ("FindFirstChild", "Find a child instance"),
            ("WaitForChild", "Wait for a child instance"),
            ("GetChildren", "Child instances"),
        ],
        _ => return Vec::new(),
    };
    let lower = prefix.to_ascii_lowercase();
    items.iter().filter(|(name, _)| name.to_ascii_lowercase().starts_with(&lower))
        .take(12).map(|(name, detail)| Completion {
            label: (*name).into(), detail: (*detail).into(), insert_text: (*name).into(),
            replace_chars: prefix.chars().count(),
        }).collect()
}

fn identifier_fragment(source_before_cursor: &str) -> &str {
    source_before_cursor.rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .next().unwrap_or("")
}

fn common_prefix_len(left: &str, right: &str) -> usize {
    left.chars().zip(right.chars()).take_while(|(a, b)| a == b).count()
}

fn lexical_completions(source_before_cursor: &str) -> Vec<Completion> {
    let prefix = identifier_fragment(source_before_cursor);
    if prefix.len() < 2 { return Vec::new(); }
    let lower = prefix.to_ascii_lowercase();
    const ITEMS: &[(&str, &str, &str)] = &[
        ("local", "Luau keyword", "local"),
        ("const", "Luau constant declaration", "const"),
        ("function", "Luau function declaration", "function"),
        ("return", "Luau keyword", "return"),
        ("export", "Luau exported declaration", "export"),
        ("type", "Luau type declaration", "type"),
        ("typeof", "Luau type operator", "typeof"),
        ("if", "Luau keyword", "if"),
        ("then", "Luau keyword", "then"),
        ("elseif", "Luau keyword", "elseif"),
        ("else", "Luau keyword", "else"),
        ("for", "Luau keyword", "for"),
        ("while", "Luau keyword", "while"),
        ("repeat", "Luau keyword", "repeat"),
        ("until", "Luau keyword", "until"),
        ("do", "Luau keyword", "do"),
        ("end", "Luau keyword", "end"),
        ("and", "Luau operator", "and"),
        ("or", "Luau operator", "or"),
        ("not", "Luau operator", "not"),
        ("true", "boolean", "true"),
        ("false", "boolean", "false"),
        ("nil", "nil value", "nil"),
        ("require", "Load a ModuleScript", "require()"),
        ("ModuleScript", "Roblox module container class", "ModuleScript"),
        ("Script", "Roblox server script class", "Script"),
        ("LocalScript", "Roblox client script class", "LocalScript"),
        ("game", "Roblox DataModel", "game"),
        ("workspace", "Roblox Workspace", "workspace"),
        ("script", "Current Roblox script", "script"),
        ("Instance.new", "Create a Roblox instance", "Instance.new(\"\")"),
        ("game:GetService", "Get a Roblox service", "game:GetService(\"\")"),
        ("task.wait", "Yield the current task", "task.wait()"),
        ("task.spawn", "Spawn a task", "task.spawn(function()\n\t\nend)"),
        ("print", "Write to Output", "print()"),
        ("warn", "Write a warning", "warn()"),
        ("pairs", "Iterate a table", "pairs()"),
        ("ipairs", "Iterate an array", "ipairs()"),
    ];
    let shared = |candidate: &str| candidate.to_ascii_lowercase().chars()
        .zip(lower.chars()).take_while(|(a, b)| a == b).count();
    let mut matches: Vec<_> = ITEMS.iter()
        .filter(|(label, _, _)| {
            let candidate = label.to_ascii_lowercase();
            candidate.starts_with(&lower) || (lower.len() >= 3 && shared(label) >= 2)
        })
        .map(|(label, detail, insert)| Completion {
            label: (*label).into(), detail: (*detail).into(), insert_text: (*insert).into(),
            replace_chars: prefix.chars().count(),
        }).collect();
    matches.sort_by_key(|item| {
        let label = item.label.to_ascii_lowercase();
        if label.starts_with(&lower) { 0 } else { 1 }
    });
    matches.truncate(12);
    matches
}

fn member_expression_at_end(source: &str) -> Option<(&str, &str)> {
    let tail = source.trim_end_matches(char::is_whitespace)
        .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.')).next()?;
    let (alias, prefix) = tail.rsplit_once('.')?;
    (is_identifier(alias) && prefix.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .then_some((alias, prefix))
}

fn module_member_owners(source: &str, member: &str) -> std::collections::BTreeSet<String> {
    source.lines().filter_map(|raw| {
        let code = raw.split("--").next().unwrap_or("").trim();
        let declaration = code.strip_prefix("function ")
            .or_else(|| code.strip_prefix("const function ")).unwrap_or(code);
        let (owner, rest) = declaration.split_once('.')?;
        (identifier_start(rest) == member && is_identifier(owner.trim()))
            .then(|| owner.trim().to_string())
    }).collect()
}

fn replace_identifier_token(source: &str, needle: &str, replacement: &str) -> (String, usize) {
    let mut output = String::with_capacity(source.len());
    let mut rest = source;
    let mut count = 0;
    while let Some(position) = rest.find(needle) {
        let before = rest[..position].chars().next_back();
        let after = rest[position + needle.len()..].chars().next();
        let needs_left_boundary = needle.chars().next().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        let needs_right_boundary = needle.chars().next_back().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        let valid = (!needs_left_boundary || !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_'))
            && (!needs_right_boundary || !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_'));
        output.push_str(&rest[..position]);
        if valid {
            output.push_str(replacement);
            count += 1;
        } else {
            output.push_str(needle);
        }
        rest = &rest[position + needle.len()..];
    }
    output.push_str(rest);
    (output, count)
}

fn fuzzy_match(haystack: &str, needle: &str) -> bool {
    if haystack.contains(needle) { return true; }
    let mut wanted = needle.chars();
    let mut next = wanted.next();
    for character in haystack.chars() {
        if next == Some(character) { next = wanted.next(); }
    }
    next.is_none()
}

fn contains_identifier(line: &str, needle: &str) -> bool {
    line.match_indices(needle).any(|(start, _)| {
        let before = line[..start].chars().next_back();
        let end = start + needle.len();
        let after = line[end..].chars().next();
        !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            && !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

fn require_string_at_cursor(source_before_cursor: &str) -> Option<&str> {
    let quote = source_before_cursor.rfind(|c| c == '"' || c == '\'')?;
    let before_quote = source_before_cursor[..quote].trim_end();
    if !before_quote.ends_with("require(") {
        return None;
    }
    Some(&source_before_cursor[quote + 1..])
}

fn relative_module_path(from_dir: &str, target: &str) -> String {
    let from: Vec<&str> = from_dir.split('/').filter(|part| !part.is_empty()).collect();
    let to: Vec<&str> = target.split('/').filter(|part| !part.is_empty()).collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts = vec![".."; from.len().saturating_sub(common)];
    parts.extend_from_slice(&to[common..]);
    let path = parts.join("/");
    if path.starts_with("../") || path == ".." { path } else { format!("./{path}") }
}

fn member_expression_at_cursor(source: &str, cursor_char: usize) -> Option<&str> {
    let chars: Vec<char> = source.chars().collect();
    let cursor = cursor_char.min(chars.len());
    let mut start = cursor;
    while start > 0 && (chars[start - 1].is_ascii_alphanumeric() || matches!(chars[start - 1], '_' | '.')) {
        start -= 1;
    }
    let mut end = cursor;
    while end < chars.len() && (chars[end].is_ascii_alphanumeric() || matches!(chars[end], '_' | '.')) {
        end += 1;
    }
    if start == end { return None }
    let start_byte = char_to_byte(source, start);
    let end_byte = char_to_byte(source, end);
    Some(&source[start_byte..end_byte])
}

fn member_definition_line(source: &str, member: &str) -> Option<usize> {
    source.lines().enumerate().find_map(|(line, raw)| {
        let code = raw.split("--").next().unwrap_or("").trim();
        let direct_member = code.split_once('.').is_some_and(|(_, rest)| {
            identifier_start(rest) == member
        });
        let exported = code.strip_prefix("export ").is_some_and(|rest| {
            identifier_start(rest.trim_start_matches("type ").trim_start_matches("function ")) == member
        });
        let returned = code.contains('=') && identifier_start(
            code.strip_prefix("return {").unwrap_or(code).trim()
        ) == member;
        (direct_member || exported || returned).then_some(line + 1)
    })
}

fn source_symbols(source: &str) -> Vec<(String, String, usize)> {
    let mut result = Vec::new();
    for (index, raw) in source.lines().enumerate() {
        let code = raw.split("--").next().unwrap_or("").trim();
        let function = code.strip_prefix("local function ")
            .or_else(|| code.strip_prefix("const function "))
            .or_else(|| code.strip_prefix("export function "))
            .or_else(|| code.strip_prefix("function "));
        if let Some(value) = function {
            let name: String = value.chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':'))
                .collect();
            if !name.is_empty() {
                let detail = function_signature(&name, &value[name.len()..])
                    .unwrap_or_else(|| format!("function {name}"));
                result.push((name, detail, index + 1));
            }
        }
        let type_decl = code.strip_prefix("export type ").or_else(|| code.strip_prefix("type "));
        if let Some(value) = type_decl {
            let name = identifier_start(value);
            if is_identifier(name) {
                result.push((name.to_string(), format!("type {name}"), index + 1));
            }
        }
    }
    // Returned fields and other module exports are not always declarations.
    for (name, detail) in exported_members(source) {
        if !result.iter().any(|(existing, _, _)| existing == &name) {
            result.push((name.clone(), detail, member_definition_line(source, &name).unwrap_or(1)));
        }
    }
    result
}

fn exported_members(source: &str) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    let mut in_return_table = false;
    for line in source.lines() {
        let code = line.split("--").next().unwrap_or("").trim();
        let declaration = code.strip_prefix("function ")
            .or_else(|| code.strip_prefix("const function ")).unwrap_or(code);
        if let Some((_, member)) = declaration.split_once('.') {
            let name = identifier_start(member);
            if is_identifier(name) {
                let suffix = &member[name.len()..];
                let signature = function_signature(name, suffix)
                    .unwrap_or_else(|| format!("field {name}"));
                result.insert(name.to_string(), signature);
            }
        }
        if let Some(rest) = code.strip_prefix("export type ") {
            let name = identifier_start(rest);
            if is_identifier(name) {
                let definition = rest.split_once('=').map_or("", |(_, value)| value.trim());
                let detail = if definition.is_empty() {
                    format!("type {name}")
                } else {
                    format!("type {name} = {definition}")
                };
                result.insert(name.to_string(), detail);
            }
        } else if let Some(rest) = code.strip_prefix("export ") {
            let function = rest.strip_prefix("function ");
            let value = function.unwrap_or(rest);
            let name = identifier_start(value);
            if is_identifier(name) {
                let detail = function
                    .and_then(|_| function_signature(name, &value[name.len()..]))
                    .unwrap_or_else(|| format!("export {name}"));
                result.insert(name.to_string(), detail);
            }
        }
        // Also understand `return { foo = value, bar = function() ... }`.
        if code.starts_with("return {") { in_return_table = true; }
        if in_return_table {
            let body = code.strip_prefix("return {").unwrap_or(code);
            for field in body.split(',') {
                let field = field.trim();
                let name = identifier_start(field);
                if field.contains('=') && is_identifier(name) {
                    let detail = field.split_once('=').map_or(
                        format!("field {name}"),
                        |(_, value)| {
                            let value = value.trim();
                            if let Some(rest) = value.strip_prefix("function") {
                                function_signature(name, rest).unwrap_or_else(|| format!("function {name}"))
                            } else {
                                format!("field {name}")
                            }
                        },
                    );
                    result.entry(name.to_string()).or_insert(detail);
                }
            }
            if code.contains('}') { in_return_table = false; }
        }
    }
    result
}

fn function_signature(name: &str, suffix: &str) -> Option<String> {
    let open = suffix.find('(')?;
    let close = suffix[open..].find(')')? + open;
    let parameters = &suffix[open..=close];
    let return_type = suffix[close + 1..].trim();
    Some(if return_type.starts_with(':') {
        format!("function {name}{parameters} {return_type}")
    } else {
        format!("function {name}{parameters}")
    })
}

fn identifier_start(value: &str) -> &str {
    value.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).next().unwrap_or("")
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_relative_paths() {
        assert_eq!(collapse_path("ReplicatedStorage/Package/Sub/../Inventory"), "ReplicatedStorage/Package/Inventory");
    }

    #[test]
    fn completes_and_applies_require_paths() {
        assert_eq!(require_string_at_cursor("const X = require(\"../Inv"), Some("../Inv"));
        assert_eq!(relative_module_path("Game/Controllers", "Game/Inventory"), "../Inventory");
        let mut source = "require(\"../Inv\")".to_string();
        let cursor = "require(\"../Inv".chars().count();
        let completion = Completion {
            label: "../Inventory".into(),
            detail: String::new(),
            insert_text: "../Inventory".into(),
            replace_chars: "../Inv".chars().count(),
        };
        apply_suggestion_at(&mut source, cursor, &completion);
        assert_eq!(source, "require(\"../Inventory\")");
    }

    #[test]
    fn suggests_luau_keywords_and_corrects_close_prefixes() {
        let exact = lexical_completions("loc");
        assert_eq!(exact.first().map(|item| item.label.as_str()), Some("local"));
        assert!(lexical_completions("lope").iter().any(|item| item.label == "local"));
    }

    #[test]
    fn dotted_roblox_completion_does_not_duplicate_game() {
        let index = ProjectIndex::default();
        let suggestions = index.complete_at(Ref::none(), "game.F", 6);
        assert!(suggestions.iter().any(|item| item.label == "FindService"));
        assert!(!suggestions.iter().any(|item| item.label == "game"));
    }

    #[test]
    fn completion_replaces_only_text_before_cursor() {
        let mut source = "Inventory.Ad + Inventory.Other".to_string();
        let cursor = "Inventory.Ad".chars().count();
        let new_cursor = apply_completion_at(&mut source, cursor, "AddItem");
        assert_eq!(source, "Inventory.AddItem + Inventory.Other");
        assert_eq!(new_cursor, "Inventory.AddItem".chars().count());
    }

    #[test]
    fn distinguishes_immediate_and_deferred_requires() {
        let uses = scan_requires(
            "const A = require(\"./A\")\nlocal function later()\n require(\"./B\")\nend",
        );
        assert_eq!(uses.len(), 2);
        assert!(!uses[0].deferred);
        assert!(uses[1].deferred);
    }

    #[test]
    fn finds_immediate_module_cycles() {
        let a = Ref::new();
        let b = Ref::new();
        let mut index = ProjectIndex::default();
        index.paths.insert(a, "A".into());
        index.paths.insert(b, "B".into());
        index.module_refs.insert("a".into(), a);
        index.module_refs.insert("b".into(), b);
        index.module_sources.insert(a, "const B = require(\"./B\")".into());
        index.module_sources.insert(b, "const A = require(\"./A\")".into());
        let warnings = index.cycle_diagnostics(a, "const B = require(\"./B\")");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("A → B → A"));
    }

    #[test]
    fn reports_unresolved_modules_and_unknown_members() {
        let mut index = ProjectIndex::default();
        index.modules.insert(
            "inventory".into(),
            BTreeMap::from([("AddItem".into(), "function AddItem()".into())]),
        );
        let source = "const Inventory = require(\"./Inventory\")\nInventory.Missing()";
        let warnings = index.diagnostics(Ref::none(), source);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("Unknown member 'Missing'"));

        let unresolved = index.diagnostics(
            Ref::none(),
            "const Missing = require(\"./DoesNotExist\")",
        );
        assert_eq!(unresolved.len(), 1);
        assert!(unresolved[0].message.contains("Unresolved module"));
    }

    #[test]
    fn locates_member_definitions() {
        let source = "local Inventory = {}\n\nfunction Inventory.AddItem() end\nexport type Item = string";
        assert_eq!(member_definition_line(source, "AddItem"), Some(3));
        assert_eq!(member_definition_line(source, "Item"), Some(4));
    }

    #[test]
    fn renames_resolved_members_without_touching_similar_names() {
        let module = Ref::new();
        let consumer = Ref::new();
        let mut index = ProjectIndex::default();
        index.paths.insert(module, "Inventory".into());
        index.paths.insert(consumer, "Controller".into());
        index.module_refs.insert("inventory".into(), module);
        index.module_sources.insert(module, "function Inventory.Add() end".into());
        index.script_sources.insert(module, "function Inventory.Add() end".into());
        index.script_sources.insert(
            consumer,
            "const Items = require(\"./Inventory\")\nItems.Add()\nItems.Additional()".into(),
        );
        let definition = Definition { referent: module, line: 1, member: Some("Add".into()) };
        let edits = index.rename_member(&definition, "Insert");
        let consumer_edit = edits.iter().find(|edit| edit.referent == consumer).unwrap();
        assert!(consumer_edit.source.contains("Items.Insert()"));
        assert!(consumer_edit.source.contains("Items.Additional()"));
    }

    #[test]
    fn reports_const_reassignment_and_literal_type_mismatch() {
        let warnings = semantic_diagnostics(
            "const LIMIT: number = 10\nLIMIT += 1\nlocal name: string = 42",
        );
        assert!(warnings.iter().any(|warning| warning.message.contains("Cannot reassign const")));
        assert!(warnings.iter().any(|warning| warning.message.contains("annotated type 'string'")));
    }

    #[test]
    fn reports_invalid_roblox_classes_properties_and_enums() {
        let warnings = semantic_diagnostics(
            "local part = Instance.new(\"DefinitelyNotAClass\")\n\
             local real = Instance.new(\"Part\")\n\
             real.Transparancy = 0.5\n\
             real.Material = Enum.Material.DefinitelyNotAnItem",
        );
        assert!(warnings.iter().any(|warning| warning.message.contains("Unknown Roblox class")));
        assert!(warnings.iter().any(|warning| warning.message.contains("Unknown property 'Transparancy'")));
        assert!(warnings.iter().any(|warning| warning.message.contains("Unknown item")));
    }

    #[test]
    fn finds_module_and_return_table_members() {
        let members = exported_members(
            "function Inventory.AddItem(player: Player, item: Item): boolean\n\
             return { MaxSlots = 20, Remove = function(item: Item) end }\n\
             export type Item = { Name: string }",
        );
        assert_eq!(
            members.get("AddItem").map(String::as_str),
            Some("function AddItem(player: Player, item: Item): boolean")
        );
        assert!(members.contains_key("MaxSlots"));
        assert!(members.contains_key("Remove"));
        assert_eq!(
            members.get("Item").map(String::as_str),
            Some("type Item = { Name: string }")
        );
    }
}
