//! Lightweight project-wide Luau intelligence for the built-in mobile editor.
//!
//! This intentionally has no filesystem or LSP dependency: it indexes the
//! Script instances already present in the loaded DataModel, resolves common
//! instance-path and string `require` forms, and exposes ModuleScript members
//! for completion. It can later be replaced or supplemented by luau-lsp.

use crate::rbxl;
use rbx_dom_weak::{types::Ref, WeakDom};
use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub label: String,
    pub detail: String,
}

#[derive(Debug, Default)]
pub struct ProjectIndex {
    /// Normalized DataModel path or module name -> exported member names.
    modules: HashMap<String, BTreeSet<String>>,
}

impl ProjectIndex {
    pub fn build(dom: &WeakDom) -> Self {
        let mut index = Self::default();
        let root = dom.root_ref();
        index.walk(dom, root, &mut Vec::new());
        index
    }

    fn walk(&mut self, dom: &WeakDom, referent: Ref, path: &mut Vec<String>) {
        let Some(instance) = dom.get_by_ref(referent) else { return };
        let is_root = referent == dom.root_ref();
        if !is_root {
            path.push(instance.name.clone());
        }

        if instance.class.as_str() == "ModuleScript" {
            let members = exported_members(&rbxl::get_source(dom, referent).unwrap_or_default());
            let full_path = path.join(".");
            self.modules.insert(normalize_path(&full_path), members.clone());
            self.modules.insert(normalize_path(&instance.name), members);
        }

        for &child in instance.children() {
            self.walk(dom, child, path);
        }
        if !is_root {
            path.pop();
        }
    }

    /// Suggest members when the caret is at the end of `Alias.partial` and
    /// Alias was assigned from require(...). This supports both DataModel paths
    /// and string requires.
    pub fn complete(&self, source: &str) -> Vec<Completion> {
        let Some((alias, prefix)) = member_expression_at_end(source) else { return Vec::new() };
        let aliases = require_aliases(source);
        let Some(module_path) = aliases.get(alias) else { return Vec::new() };
        let key = normalize_path(module_path);
        let members = self.modules.get(&key).or_else(|| {
            key.rsplit('.').next().and_then(|name| self.modules.get(name))
        });
        let Some(members) = members else { return Vec::new() };
        members
            .iter()
            .filter(|member| member.starts_with(prefix))
            .take(12)
            .map(|member| Completion {
                label: member.clone(),
                detail: format!("{alias} member · {module_path}"),
            })
            .collect()
    }
}

/// Replace only the member fragment at the end of a source buffer.
pub fn apply_completion(source: &mut String, member: &str) {
    let prefix_len = source
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .map(char::len_utf8)
        .sum::<usize>();
    source.truncate(source.len().saturating_sub(prefix_len));
    source.push_str(member);
}

fn normalize_path(path: &str) -> String {
    let mut value = path.trim().trim_matches(['"', '\'', ' ']).replace('/', ".");
    for prefix in ["game.", "Game."] {
        if value.starts_with(prefix) {
            value = value[prefix.len()..].to_string();
        }
    }
    value.replace(":GetService(\"", ".")
        .replace("\")", "")
        .to_ascii_lowercase()
}

fn require_aliases(source: &str) -> HashMap<&str, String> {
    let mut aliases = HashMap::new();
    for line in source.lines() {
        let code = line.split("--").next().unwrap_or("").trim();
        let code = code.strip_prefix("local ").or_else(|| code.strip_prefix("const "));
        let Some(code) = code else { continue };
        let Some((alias, rhs)) = code.split_once('=') else { continue };
        let alias = alias.trim();
        if !is_identifier(alias) { continue }
        let rhs = rhs.trim();
        let Some(inner) = rhs.strip_prefix("require(").and_then(|s| s.strip_suffix(')')) else { continue };
        aliases.insert(alias, require_path(inner.trim()));
    }
    aliases
}

fn require_path(expression: &str) -> String {
    let quoted = expression.trim_matches(['"', '\'']);
    if quoted != expression {
        return quoted.to_string();
    }
    expression
        .replace(":WaitForChild(\"", ".")
        .replace(":FindFirstChild(\"", ".")
        .replace("\")", "")
        .replace("script.Parent.", "")
}

fn member_expression_at_end(source: &str) -> Option<(&str, &str)> {
    let tail = source
        .trim_end_matches(|c: char| c.is_whitespace())
        .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
        .next()?;
    let (alias, prefix) = tail.rsplit_once('.')?;
    if is_identifier(alias) && prefix.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Some((alias, prefix))
    } else {
        None
    }
}

fn exported_members(source: &str) -> BTreeSet<String> {
    let mut result = BTreeSet::new();
    for line in source.lines() {
        let code = line.split("--").next().unwrap_or("").trim();
        let code = code.strip_prefix("function ").or_else(|| code.strip_prefix("const function ")).unwrap_or(code);
        if let Some((_, member)) = code.split_once('.') {
            let name = member.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).next().unwrap_or("");
            if is_identifier(name) { result.insert(name.to_string()); }
        }
        if let Some(rest) = code.strip_prefix("export ") {
            let name = rest.trim_start_matches("function ")
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).next().unwrap_or("");
            if is_identifier(name) { result.insert(name.to_string()); }
        }
    }
    result
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
    fn resolves_string_require_members() {
        let mut index = ProjectIndex::default();
        index.modules.insert("inventory".into(), BTreeSet::from(["AddItem".into(), "MaxSlots".into()]));
        let result = index.complete("const Inventory = require(\"Inventory\")\nInventory.Ad");
        assert_eq!(result.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(), ["AddItem"]);
    }

    #[test]
    fn finds_module_table_members() {
        let members = exported_members("function Inventory.AddItem() end\nInventory.MaxSlots = 20");
        assert!(members.contains("AddItem"));
        assert!(members.contains("MaxSlots"));
    }
}
