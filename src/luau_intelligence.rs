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

#[derive(Debug, Default)]
pub struct ProjectIndex {
    /// Normalized DataModel path or unambiguous module name -> members.
    modules: HashMap<String, BTreeMap<String, String>>,
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

        if instance.class.as_str() == "ModuleScript" {
            let members = exported_members(&rbxl::get_source(dom, referent).unwrap_or_default());
            self.modules.insert(normalize_path(&path.join("/")), members.clone());
            self.modules.insert(normalize_path(&instance.name), members);
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
        diagnostics.sort_by_key(|diagnostic| diagnostic.line);
        diagnostics.dedup();
        diagnostics
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
