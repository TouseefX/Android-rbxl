//! Lightweight project-wide Luau intelligence for the built-in mobile editor.
//!
//! This indexes Script instances in the local DataModel, resolves instance and
//! modern string requires (`./`, `../`, and `@self`), and exposes ModuleScript
//! members without requiring a filesystem or an external language server.

use crate::rbxl;
use rbx_dom_weak::{types::Ref, WeakDom};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub label: String,
    pub detail: String,
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

#[derive(Debug, Default)]
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
    pub fn build(dom: &WeakDom) -> Self {
        let mut index = Self::default();
        index.walk(dom, dom.root_ref(), &mut Vec::new());
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

    /// Complete `Alias.partial` at a character cursor position, resolving Alias
    /// from a require declaration anywhere in the file.
    pub fn complete_at(
        &self,
        current_script: Ref,
        source: &str,
        cursor_char: usize,
    ) -> Vec<Completion> {
        let cursor_byte = char_to_byte(source, cursor_char);
        let Some((alias, prefix)) = member_expression_at_end(&source[..cursor_byte]) else {
            return Vec::new();
        };
        let aliases = require_aliases(source);
        let Some(request) = aliases.get(alias) else { return Vec::new() };
        let resolved = self.resolve_request(current_script, request);
        let key = normalize_path(&resolved);
        let members = self.modules.get(&key).or_else(|| {
            key.rsplit('/').next().and_then(|name| self.modules.get(name))
        });
        let Some(members) = members else { return Vec::new() };
        members
            .iter()
            .filter(|(member, _)| member.starts_with(prefix))
            .take(12)
            .map(|(member, signature)| Completion {
                label: member.clone(),
                detail: format!("{signature}  ·  {request} → {resolved}"),
            })
            .collect()
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

fn member_expression_at_end(source: &str) -> Option<(&str, &str)> {
    let tail = source.trim_end_matches(char::is_whitespace)
        .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.')).next()?;
    let (alias, prefix) = tail.rsplit_once('.')?;
    (is_identifier(alias) && prefix.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .then_some((alias, prefix))
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
