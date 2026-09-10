//! Lightweight project-wide Luau intelligence for the built-in mobile editor.
//!
//! This indexes Script instances in the local DataModel, resolves instance and
//! modern string requires (`./`, `../`, and `@self`), and exposes ModuleScript
//! members without requiring a filesystem or an external language server.

use crate::roblox_api_data as api;
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
    /// Dot-separated fields from nested returned tables (e.g. Moves.Base).
    module_member_paths: HashMap<Ref, BTreeMap<String, String>>,
    script_sources: HashMap<Ref, String>,
    /// Every script's slash-separated virtual path in the DataModel.
    paths: HashMap<Ref, String>,
    /// Every instance's class, for path-based type resolution.
    instance_classes: HashMap<Ref, String>,
    /// Parent -> (child name, child ref) in DataModel order.
    instance_children: HashMap<Ref, Vec<(String, Ref)>>,
    /// Slash path ("ReplicatedStorage/Remotes/Hit") -> ref ("" is the root).
    path_refs: HashMap<String, Ref>,
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
                index.module_member_paths.insert(referent, returned_member_paths(source));
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
        // Path-based intelligence: every node's class plus the parent->child
        // links, so `game.ReplicatedStorage.ClientRenderRequest` resolves to
        // the real instance's class without annotations.
        if is_root {
            self.path_refs.insert(String::new(), referent);
        } else {
            self.path_refs.insert(path.join("/"), referent);
        }
        self.instance_classes.insert(referent, instance.class.clone());
        for &child in instance.children() {
            if let Some(node) = dom.get_by_ref(child) {
                self.instance_children
                    .entry(referent)
                    .or_default()
                    .push((node.name.clone(), child));
            }
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
            self.module_member_paths.insert(referent, returned_member_paths(&source));
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

    /// Complete `Alias.partial` at a character cursor position. Bare
    /// identifiers only offer locals that are actually in scope at the caret
    /// (plus Luau keywords/globals) — a ModuleScript is never offered as a
    /// free variable unless the script has bound it with `local X = require(…)`.
    pub fn complete_at(
        &self,
        current_script: Ref,
        source: &str,
        cursor_char: usize,
    ) -> Vec<Completion> {
        let cursor_byte = char_to_byte(source, cursor_char);
        let locals = local_bindings_at(self, source, cursor_char, current_script);
        if let Some(typed) = require_string_at_cursor(&source[..cursor_byte]) {
            return self.complete_require_path(current_script, typed);
        }
        if let Some((root, parent, prefix, nested_separator)) =
            nested_member_expression_at_end(&source[..cursor_byte])
        {
            // `Enum.KeyCode.K` — enum container completes to enum names, and
            // an enum name completes to its items.
            if root.eq_ignore_ascii_case("Enum") {
                if parent.contains('.') {
                    return Vec::new();
                }
                return if api::enum_items(&parent).is_some() {
                    enum_item_completions(&parent, prefix)
                } else {
                    enum_name_completions(prefix)
                };
            }
            // Type-flow through API members: `player.CharacterAdded.Co` →
            // Player → CharacterAdded (RBXScriptSignal) → Connect/Wait.
            let mut root_inferred = innermost_local(&locals, root)
                .map(|binding| InferredType {
                    class: binding.instance_class.clone(),
                    path: binding.instance_path.clone(),
                })
                .unwrap_or_default();
            if root_inferred.is_unknown() {
                if let Some(api_type) = api::global_type(root) {
                    if !api_type.starts_with("@lib:") && api_type != "@enum" {
                        root_inferred.class = Some(api_type.to_string());
                        root_inferred.path = self.global_path(root, api_type, current_script);
                    }
                }
            }
            let resolved = parent_type_for_path(self, &root_inferred, &parent);
            if let Some(class) = &resolved.class {
                let mut members = api_member_completions(class, prefix, nested_separator);
                if nested_separator == '.' {
                    if let Some(location) = &resolved.path {
                        merge_child_completions(
                            &mut members,
                            self.dom_child_completions(location, prefix),
                        );
                    }
                }
                if !members.is_empty() {
                    return members;
                }
            }
            let aliases = require_aliases(source);
            // Only resolve modules that are actually bound in this script's
            // scope. Previously a module name alone (`Module.Create().x`) was
            // treated as if the script required it, which invented members for
            // names the script never declared.
            let target = innermost_local(&locals, root).is_some()
                .then(|| aliases.get(root))
                .flatten()
                .and_then(|request| self.resolve_module_ref(current_script, request));
            if let Some(paths) = target.and_then(|referent| self.module_member_paths.get(&referent)) {
                let wanted = format!("{parent}.");
                let lower = prefix.to_ascii_lowercase();
                let mut seen = std::collections::BTreeSet::new();
                return paths.iter().filter_map(|(path, detail)| {
                    let rest = path.strip_prefix(&wanted)?;
                    let child = rest.split('.').next()?;
                    if child.to_ascii_lowercase().starts_with(&lower) && seen.insert(child.to_string()) {
                        Some(Completion { label: child.into(), detail: detail.clone(), insert_text: child.into(), replace_chars: prefix.chars().count() })
                    } else { None }
                }).take(12).collect();
            }
            return Vec::new();
        }
        if let Some((alias, prefix, separator)) = member_expression_at_end(&source[..cursor_byte]) {
            let aliases = require_aliases(source);
            let local = innermost_local(&locals, alias);
            if let Some(request) = local.and_then(|_| aliases.get(alias)) {
                let resolved = self.resolve_request(current_script, request);
                let key = normalize_path(&resolved);
                let members = self.modules.get(&key).or_else(|| {
                    key.rsplit('/').next().and_then(|name| self.modules.get(name))
                });
                if let Some(members) = members {
                    return members
                        .iter()
                        .filter(|(member, detail)| member.to_ascii_lowercase().starts_with(&prefix.to_ascii_lowercase())
                            && member_matches_access(detail, separator))
                        .take(12)
                        .map(|(member, signature)| Completion {
                            label: member.clone(),
                            detail: format!("{signature}  ·  {request} → {resolved}  ·  local variable"),
                            insert_text: member.clone(),
                            replace_chars: prefix.chars().count(),
                        })
                        .collect();
                }
            }
            // Infer objects created by a module constructor:
            // `local weld = Module.new(args)` makes `weld.` use that module's
            // returned API instead of being treated as an unknown plain table.
            if let Some(module_alias) = local.and_then(|_| constructor_module_alias(source, alias)) {
                if let Some(request) = aliases.get(module_alias) {
                    let target = self.resolve_module_ref(current_script, request);
                    if let Some(members) = target.and_then(|target| self.module_sources.get(&target))
                        .map(|source| constructed_object_members(source))
                    {
                        return members.into_iter()
                            .filter(|(member, detail)| member.to_ascii_lowercase().starts_with(&prefix.to_ascii_lowercase())
                                && member_matches_access(detail, separator))
                            .take(12)
                            .map(|(member, detail)| Completion {
                                label: member.clone(),
                                detail: format!("constructed {module_alias} object · {detail}"),
                                insert_text: member,
                                replace_chars: prefix.chars().count(),
                            }).collect();
                    }
                }
            }

            // Typed locals (annotations, Instance.new, GetService, property
            // chains) get kind-aware API members: properties/events/statics
            // through `.`, methods ONLY through `:` — `part.Destroy` is never
            // offered, `part:Destroy` is. A plain unknown local must not fall
            // back to a ModuleScript of the same name, because
            // `local Debris = {}` is not the Debris module.
            if let Some(binding) = local {
                if let Some(class_name) = &binding.instance_class {
                    let mut members = api_member_completions(class_name, prefix, separator);
                    // A local with a DataModel location also offers the
                    // real children (`local rs = ...ReplicatedStorage` ->
                    // `rs.ClientRenderRequest`).
                    if separator == '.' {
                        if let Some(location) = &binding.instance_path {
                            merge_child_completions(
                                &mut members,
                                self.dom_child_completions(location, prefix),
                            );
                        }
                    }
                    if !members.is_empty() {
                        return members;
                    }
                    if separator == '.' {
                        let properties: Vec<_> = schema::get_class_schema_properties(class_name)
                            .into_iter()
                            .filter(|(name, _)| name.to_ascii_lowercase().starts_with(&prefix.to_ascii_lowercase()))
                            .take(12)
                            .map(|(name, detail)| Completion {
                                label: name.clone(),
                                detail: format!("{class_name} property · {detail}"),
                                insert_text: name,
                                replace_chars: prefix.chars().count(),
                            })
                            .collect();
                        if !properties.is_empty() {
                            return properties;
                        }
                    }
                }
                // Local shadows any built-in of the same name; never invent
                // members for an untyped local.
                return Vec::new();
            }

            // No local: built-in globals/services/datatypes/libraries/enums
            // complete from the generated API data (kind-aware). A bare module
            // name is still never resolved — it must be require()d.
            return global_member_completions(self, alias, prefix, separator, current_script);
        }
        let before_cursor = &source[..cursor_byte];
        // Nested member chains (e.g. Module.Factory().value) need type-flow
        // information. Never offer lexical keywords in the middle of one:
        // accepting those was the source of duplicated `game.game.f` text.
        let expression_tail = before_cursor
            .rsplit(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | ',' | '='))
            .next().unwrap_or("");
        if expression_tail.contains('.') {
            return Vec::new();
        }
        let mut completions = Vec::new();
        let prefix = identifier_fragment(before_cursor);
        if prefix.len() >= 2 {
            let lower = prefix.to_ascii_lowercase();
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            let aliases = require_aliases(source);

            // In-scope locals win over keywords: a local `game` shadows the
            // Roblox global, and `local Debris = require(...)` is the only way
            // a module name is offered as a bare identifier. ModuleScripts are
            // never invented as locals.
            for binding in &locals {
                let candidate = binding.name.to_ascii_lowercase();
                if (candidate.starts_with(&lower)
                    || (lower.len() >= 3 && common_prefix_len(&candidate, &lower) >= 2))
                    && seen.insert(candidate)
                {
                    let detail = if let Some(request) = aliases.get(binding.name.as_str()) {
                        format!(
                            "{}  ·  ModuleScript → {}",
                            binding.detail,
                            self.resolve_request(current_script, request),
                        )
                    } else {
                        binding.detail.clone()
                    };
                    completions.push(Completion {
                        label: binding.name.clone(),
                        detail,
                        insert_text: binding.name.clone(),
                        replace_chars: prefix.chars().count(),
                    });
                }
            }
            // Generated Roblox globals/datatypes/libraries (Vector3, task,
            // game, script, ...) — never ModuleScript names.
            for &(label, detail, insert) in api::BARE_GLOBALS {
                let candidate = label.to_ascii_lowercase();
                if (candidate.starts_with(&lower)
                    || (lower.len() >= 3 && common_prefix_len(&candidate, &lower) >= 2))
                    && seen.insert(candidate)
                {
                    completions.push(Completion {
                        label: label.to_string(),
                        detail: detail.to_string(),
                        insert_text: insert.to_string(),
                        replace_chars: prefix.chars().count(),
                    });
                }
            }
            for item in lexical_completions(before_cursor) {
                if seen.insert(item.label.to_ascii_lowercase()) {
                    completions.push(item);
                }
            }
            // ModuleScript names that are not bound as locals are deliberately
            // not offered; typing `Debris` must not insert a module name into
            // a script that never require()d it.
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

            // Only report unknown members when the target module's public
            // shape is actually knowable. Modules that build their exports
            // dynamically (`M[name] = ...`), forward another module, or
            // return a bare function have no statically complete member list,
            // and flagging every access against an empty/partial map produced
            // a wall of false "Unknown member" warnings.
            let module_source = self.resolve_module_ref(current_script, &request)
                .and_then(|referent| self.module_sources.get(&referent));
            let shape_is_known = !members.is_empty()
                && !module_source.is_some_and(|source| has_dynamic_exports(source));
            if !shape_is_known {
                continue;
            }

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

    /// Ref of the DataModel node at `path` ("" is the root), if the open
    /// place actually contains it.
    fn dom_ref_at_path(&self, path: &str) -> Option<Ref> {
        self.path_refs.get(path).copied()
    }

    /// Class of the DataModel node at `path`.
    fn dom_class_at_path(&self, path: &str) -> Option<&str> {
        let referent = self.dom_ref_at_path(path)?;
        self.instance_classes.get(&referent).map(String::as_str)
    }

    /// One exact-name child step: (child path, child class). Member access
    /// in Luau is case-sensitive, so this match is too.
    fn dom_child(&self, parent_path: &str, name: &str) -> Option<(String, String)> {
        let parent = self.dom_ref_at_path(parent_path)?;
        let children = self.instance_children.get(&parent)?;
        let (child_name, child_ref) = children
            .iter()
            .find(|(child_name, _)| child_name.as_str() == name)?;
        let class = self.instance_classes.get(child_ref)?.clone();
        let child_path = if parent_path.is_empty() {
            child_name.clone()
        } else {
            format!("{parent_path}/{child_name}")
        };
        Some((child_path, class))
    }

    /// Children of a DataModel node as (name, class) in tree order.
    fn dom_children(&self, path: &str) -> Vec<(String, String)> {
        let Some(parent) = self.dom_ref_at_path(path) else {
            return Vec::new();
        };
        let Some(children) = self.instance_children.get(&parent) else {
            return Vec::new();
        };
        children
            .iter()
            .filter_map(|(name, referent)| {
                self.instance_classes
                    .get(referent)
                    .map(|class| (name.clone(), class.clone()))
            })
            .collect()
    }

    /// Children of a DataModel node as dot-only completions, so
    /// `game.ReplicatedStorage` and `ReplicatedStorage.Things` complete from
    /// the open place instead of needing annotations.
    fn dom_child_completions(&self, path: &str, prefix: &str) -> Vec<Completion> {
        let lower = prefix.to_ascii_lowercase();
        self.dom_children(path)
            .into_iter()
            .filter(|(name, _)| name.to_ascii_lowercase().starts_with(&lower))
            .take(12)
            .map(|(name, class)| {
                let child_path = if path.is_empty() {
                    name.clone()
                } else {
                    format!("{path}/{name}")
                };
                Completion {
                    label: name.clone(),
                    detail: format!("{class} \u{b7} {child_path}"),
                    insert_text: name,
                    replace_chars: prefix.chars().count(),
                }
            })
            .collect()
    }

    /// DataModel location of a built-in global: `game` is the root,
    /// `script` is the script being edited, and service globals
    /// (`workspace`, ...) live at the root under their class name.
    fn global_path(&self, global: &str, api_type: &str, current_script: Ref) -> Option<String> {
        if global == "game" {
            return Some(String::new());
        }
        if global == "script" {
            return self.paths.get(&current_script).cloned();
        }
        if schema::class_is_service(api_type) {
            return Some(api_type.to_string());
        }
        None
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

/// Kind-aware completions for a known API type: properties/events/statics
/// complete through `.`; methods complete ONLY through `:` (Roblox methods
/// are colon functions — `part.Destroy` is never offered).
fn api_member_completions(owner_type: &str, prefix: &str, separator: char) -> Vec<Completion> {
    let lower = prefix.to_ascii_lowercase();
    let owner = owner_type.strip_prefix("Enum:").unwrap_or(owner_type);
    api::members_of(owner)
        .into_iter()
        .filter(|(name, _, kind, _)| {
            api::kind_matches_separator(*kind, separator)
                && name.to_ascii_lowercase().starts_with(&lower)
        })
        .take(12)
        .map(|(name, detail, _, _)| Completion {
            label: name.to_string(),
            detail: format!("{detail}  ·  {owner_type}"),
            insert_text: name.to_string(),
            replace_chars: prefix.chars().count(),
        })
        .collect()
}

/// Members of the built-in globals/services/datatypes/libraries (`game`,
/// `workspace`, `script`, `Vector3`, `task`, `Enum`, ...) from the generated
/// API tables. Kind-aware: methods only via `:`.
fn global_member_completions(
    index: &ProjectIndex,
    owner: &str,
    prefix: &str,
    separator: char,
    current_script: Ref,
) -> Vec<Completion> {
    if owner.eq_ignore_ascii_case("Enum") {
        return enum_name_completions(prefix);
    }
    let Some(api_type) = api::global_type(owner) else {
        return Vec::new();
    };
    if let Some(library) = api_type.strip_prefix("@lib:") {
        let lower = prefix.to_ascii_lowercase();
        let Some(members) = api::library(library) else {
            return Vec::new();
        };
        return members
            .iter()
            .filter(|(name, _, kind, _)| {
                api::kind_matches_separator(*kind, separator)
                    && name.to_ascii_lowercase().starts_with(&lower)
            })
            .take(12)
            .map(|(name, detail, _, _)| Completion {
                label: name.to_string(),
                detail: format!("{detail}  ·  {library}"),
                insert_text: name.to_string(),
                replace_chars: prefix.chars().count(),
            })
            .collect();
    }
    if api_type == "@enum" {
        return Vec::new();
    }
    let path = index.global_path(owner, api_type, current_script);
    // `script` takes the class of the script being edited.
    let class_name = if owner == "script" {
        path.as_deref()
            .and_then(|script_path| index.dom_class_at_path(script_path))
            .unwrap_or(api_type)
            .to_string()
    } else {
        api_type.to_string()
    };
    let mut members = api_member_completions(&class_name, prefix, separator);
    // DataModel children (`game.ReplicatedStorage`, `workspace.Baseplate`)
    // complete through `.` alongside the API members.
    if separator == '.' {
        if let Some(location) = &path {
            merge_child_completions(&mut members, index.dom_child_completions(location, prefix));
        }
    }
    members
}

/// `Enum.` → enum type names.
fn enum_name_completions(prefix: &str) -> Vec<Completion> {
    let lower = prefix.to_ascii_lowercase();
    let mut names: Vec<&'static str> = api::ENUMS
        .iter()
        .filter(|(name, _)| name.to_ascii_lowercase().starts_with(&lower))
        .map(|(name, _)| *name)
        .collect();
    names.sort_by_key(|name| name.to_ascii_lowercase());
    names.truncate(12);
    names
        .into_iter()
        .map(|name| Completion {
            label: name.to_string(),
            detail: format!("Roblox enum · {name}"),
            insert_text: name.to_string(),
            replace_chars: prefix.chars().count(),
        })
        .collect()
}

/// `Enum.KeyCode.` → enum items.
fn enum_item_completions(enum_name: &str, prefix: &str) -> Vec<Completion> {
    let lower = prefix.to_ascii_lowercase();
    let mut items: Vec<&'static str> = api::enum_items(enum_name)
        .map(|items| {
            items
                .iter()
                .filter(|item| item.to_ascii_lowercase().starts_with(&lower))
                .copied()
                .collect()
        })
        .unwrap_or_default();
    items.sort();
    items.truncate(12);
    items
        .into_iter()
        .map(|item| Completion {
            label: item.to_string(),
            detail: format!("Enum.{enum_name} · {item}"),
            insert_text: item.to_string(),
            replace_chars: prefix.chars().count(),
        })
        .collect()
}

/// Appends DataModel children to API members, skipping names the API
/// already offered (`game.Workspace` exists in both tables).
fn merge_child_completions(members: &mut Vec<Completion>, children: Vec<Completion>) {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for member in members.iter() {
        seen.insert(member.label.to_ascii_lowercase());
    }
    for child in children {
        if seen.insert(child.label.to_ascii_lowercase()) {
            members.push(child);
        }
    }
    members.truncate(12);
}

/// Walks a dotted parent path (`CharacterAdded`, `ReplicatedStorage.Hit`,
/// or `Moves.Items`) through DataModel children and API member types,
/// returning the final owner type for member completion.
fn parent_type_for_path(index: &ProjectIndex, root: &InferredType, parent: &str) -> InferredType {
    let mut current = root.clone();
    for segment in parent.split('.') {
        // Nested chains are plain `.name` steps (no calls).
        if !infer_step(index, &mut current, segment, false, "") {
            return InferredType::default();
        }
    }
    current
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
            (candidate != lower && candidate.starts_with(&lower))
                || (lower.len() >= 3 && candidate != lower && shared(label) >= 2)
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

/// A local binding that is in scope at the completion caret.
#[derive(Debug, Clone)]
struct LocalBinding {
    name: String,
    detail: String,
    line: usize,
    /// Roblox class when the binding's type is known (`Instance.new("Part")`
    /// or a typed annotation such as `local part: BasePart`).
    instance_class: Option<String>,
    /// DataModel path when the value's location is known
    /// (`game:GetService("ReplicatedStorage")` -> `"ReplicatedStorage"`).
    instance_path: Option<String>,
}

#[derive(Debug, Clone)]
struct RawBinding {
    name: String,
    annotation: Option<String>,
    instance_class: Option<String>,
    instance_path: Option<String>,
    line: usize,
}

#[derive(Debug, Default)]
struct LocalScope {
    bindings: Vec<LocalBinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScopeKind {
    File,
    Block,
    Function,
    Loop,
    Repeat,
    IfBranch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token<'a> {
    Word(&'a str),
    Op(&'static str),
    Newline,
}

const MULTI_CHAR_OPS: &[&str] = &[
    "...", "//=", "..=", "->", "::", "==", "~=", "<=", ">=", "..", "//", "+=", "-=", "*=", "/=",
    "%=", "^=", "<<", ">>",
];

fn single_operator(byte: u8) -> &'static str {
    match byte {
        b'(' => "(",
        b')' => ")",
        b'[' => "[",
        b']' => "]",
        b'{' => "{",
        b'}' => "}",
        b',' => ",",
        b';' => ";",
        b':' => ":",
        b'.' => ".",
        b'<' => "<",
        b'>' => ">",
        b'=' => "=",
        b'+' => "+",
        b'-' => "-",
        b'*' => "*",
        b'/' => "/",
        b'%' => "%",
        b'^' => "^",
        b'#' => "#",
        b'|' => "|",
        b'&' => "&",
        b'~' => "~",
        b'?' => "?",
        b'@' => "@",
        _ => "",
    }
}

/// Returns the number of `=` signs of a long bracket starting at `at`
/// (`[[`, `[==[` …), or `None` when `[` is not a long-bracket opener.
fn long_bracket_level(source: &str, at: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes.get(at) != Some(&b'[') {
        return None;
    }
    let mut index = at + 1;
    while bytes.get(index) == Some(&b'=') {
        index += 1;
    }
    (bytes.get(index) == Some(&b'[')).then_some(index - at - 1)
}

fn skip_long_bracket(source: &str, index: &mut usize, line: &mut usize, level: usize) {
    let closer = format!("]{}]", "=".repeat(level));
    let rest = &source[*index..];
    if let Some(position) = rest.find(closer.as_str()) {
        *line += rest[..position].bytes().filter(|byte| *byte == b'\n').count();
        *index += position + closer.len();
    } else {
        *line += rest.bytes().filter(|byte| *byte == b'\n').count();
        *index = source.len();
    }
}

/// Skips a `--` comment (line or long-bracket form), keeping `line` accurate.
fn skip_comment(source: &str, index: &mut usize, line: &mut usize) {
    if let Some(level) = long_bracket_level(source, *index) {
        *index += 2 + level;
        skip_long_bracket(source, index, line, level);
        return;
    }
    while let Some(&byte) = source.as_bytes().get(*index) {
        if byte == b'\n' {
            return;
        }
        *index += 1;
    }
}

fn skip_quoted_string(source: &str, index: &mut usize, line: &mut usize) {
    let bytes = source.as_bytes();
    let quote = bytes[*index];
    *index += 1;
    while let Some(&byte) = bytes.get(*index) {
        match byte {
            b'\\' => {
                if bytes.get(*index + 1) == Some(&b'\n') {
                    *line += 1;
                    *index += 2;
                } else if bytes.get(*index + 1).is_some() {
                    *index += 2;
                } else {
                    *index += 1;
                }
            }
            b'\n' => return, // unterminated; the newline token is handled by the caller
            _ if byte == quote => {
                *index += 1;
                return;
            }
            _ => *index += 1,
        }
    }
}

/// Skips a Luau interpolated string (`` `...` ``). The `{...}` holes can
/// contain real code, but never declarations, so the whole string is trivia
/// for scope purposes.
fn skip_interpolated_string(source: &str, index: &mut usize, line: &mut usize) {
    let bytes = source.as_bytes();
    *index += 1; // opening backtick
    let mut braces = 0i32;
    while let Some(&byte) = bytes.get(*index) {
        match byte {
            b'\\' => {
                if bytes.get(*index + 1) == Some(&b'\n') {
                    *line += 1;
                    *index += 2;
                } else if bytes.get(*index + 1).is_some() {
                    *index += 2;
                } else {
                    *index += 1;
                }
            }
            b'{' => braces += 1,
            b'}' if braces > 0 => braces -= 1,
            b'`' if braces == 0 => {
                *index += 1;
                return;
            }
            b'"' | b'\'' if braces > 0 => skip_quoted_string(source, index, line),
            b'\n' => *line += 1,
            _ => {}
        }
        *index += 1;
    }
}

fn skip_number(source: &str, index: &mut usize) {
    let bytes = source.as_bytes();
    let mut i = *index;
    if bytes.get(i) == Some(&b'0')
        && bytes
            .get(i + 1)
            .copied()
            .is_some_and(|byte| matches!(byte, b'x' | b'X' | b'b' | b'B' | b'o' | b'O'))
    {
        i += 2;
        while bytes.get(i).is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_') {
            i += 1;
        }
    } else {
        while bytes.get(i).is_some_and(|byte| byte.is_ascii_digit() || *byte == b'_') {
            i += 1;
        }
        if bytes.get(i) == Some(&b'.') {
            i += 1;
            while bytes.get(i).is_some_and(|byte| byte.is_ascii_digit() || *byte == b'_') {
                i += 1;
            }
        }
        if bytes.get(i).copied().is_some_and(|byte| matches!(byte, b'e' | b'E')) {
            let mut exponent = i + 1;
            if bytes.get(exponent).copied().is_some_and(|byte| matches!(byte, b'+' | b'-')) {
                exponent += 1;
            }
            if bytes.get(exponent).is_some_and(|byte| byte.is_ascii_digit()) {
                i = exponent;
                while bytes.get(i).is_some_and(|byte| byte.is_ascii_digit() || *byte == b'_') {
                    i += 1;
                }
            }
        }
    }
    *index = i;
}

/// One lexical token, skipping comments, strings, numbers, and whitespace.
fn next_token<'a>(source: &'a str, index: &mut usize, line: &mut usize) -> Option<Token<'a>> {
    let bytes = source.as_bytes();
    loop {
        let i = *index;
        let Some(&byte) = bytes.get(i) else { return None };
        match byte {
            b'\n' => {
                *index += 1;
                *line += 1;
                return Some(Token::Newline);
            }
            b' ' | b'\t' | b'\r' | 0x0b | 0x0c => *index += 1,
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                *index += 2;
                skip_comment(source, index, line);
            }
            b'"' | b'\'' => skip_quoted_string(source, index, line),
            b'`' => skip_interpolated_string(source, index, line),
            b'[' => {
                if let Some(level) = long_bracket_level(source, i) {
                    *index += 2 + level;
                    skip_long_bracket(source, index, line, level);
                } else {
                    *index += 1;
                    return Some(Token::Op("["));
                }
            }
            b'0'..=b'9' => skip_number(source, index),
            b'.' if bytes.get(i + 1).is_some_and(|byte| byte.is_ascii_digit()) => {
                skip_number(source, index);
            }
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => {
                let start = i;
                *index += 1;
                while bytes
                    .get(*index)
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                {
                    *index += 1;
                }
                return Some(Token::Word(&source[start..*index]));
            }
            _ => {
                let rest = &source[i..];
                if let Some(&operator) = MULTI_CHAR_OPS.iter().find(|op| rest.starts_with(*op)) {
                    *index += operator.len();
                    return Some(Token::Op(operator));
                }
                let operator = single_operator(byte);
                if !operator.is_empty() {
                    *index += 1;
                    return Some(Token::Op(operator));
                }
                *index += 1;
            }
        }
    }
}

fn skip_whitespace(source: &str, mut index: usize, line: &mut usize) -> usize {
    let bytes = source.as_bytes();
    while let Some(&byte) = bytes.get(index) {
        if byte == b'\n' {
            *line += 1;
            index += 1;
        } else if byte.is_ascii_whitespace() {
            index += 1;
        } else {
            break;
        }
    }
    index
}

fn is_identifier_start(byte: Option<u8>) -> bool {
    byte.is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
}

fn read_identifier_end(source: &str, mut index: usize) -> usize {
    while source
        .as_bytes()
        .get(index)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        index += 1;
    }
    index
}

/// Reads a type annotation starting right after `:`. Returns the first bare
/// identifier of the type (so `BasePart` is usable for property completion)
/// and the index just after the annotation, stopping at `,`, `=`, `)`, or `;`
/// at bracket depth zero.
fn skip_type_annotation(source: &str, index: usize, line: &mut usize) -> (Option<String>, usize) {
    let bytes = source.as_bytes();
    let mut i = skip_whitespace(source, index, line);
    let mut first_identifier = None;
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    while i < source.len() {
        let byte = bytes[i];
        if let Some(active) = quote {
            if byte == b'\\' {
                if bytes.get(i + 1) == Some(&b'\n') {
                    *line += 1;
                }
                i += 2;
                continue;
            }
            // An unterminated string inside the type ends at the newline; stop
            // WITHOUT consuming it so the scope scanner still sees the Newline
            // token and commits the `local name` declaration.
            if byte == b'\n' {
                break;
            }
            if byte == active {
                quote = None;
            }
            i += 1;
            continue;
        }
        match byte {
            b'"' | b'\'' | b'`' => quote = Some(byte),
            b'(' | b'[' | b'{' | b'<' => depth += 1,
            b')' | b']' | b'}' | b'>' if depth > 0 => depth -= 1,
            b')' | b',' | b'=' | b';' if depth == 0 => break,
            b'-' if bytes.get(i + 1) == Some(&b'-') => skip_comment(source, &mut i, line),
            b'\n' => break,
            _ if is_identifier_start(Some(byte)) && first_identifier.is_none() && depth == 0 => {
                let start = i;
                i = read_identifier_end(source, i);
                first_identifier = Some(source[start..i].to_string());
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    (first_identifier, i)
}

/// Skips an expression (function-parameter default) until `,` or `)` at depth
/// zero, honoring strings/comments so their contents cannot end it early.
fn skip_expression(source: &str, index: usize, line: &mut usize) -> usize {
    let bytes = source.as_bytes();
    let mut i = skip_whitespace(source, index, line);
    let mut depth = 0i32;
    while i < source.len() {
        let byte = bytes[i];
        match byte {
            b'"' | b'\'' | b'`' => {
                let mut cursor = i;
                if byte == b'`' {
                    skip_interpolated_string(source, &mut cursor, line);
                } else {
                    skip_quoted_string(source, &mut cursor, line);
                }
                i = cursor;
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' if depth > 0 => depth -= 1,
            b')' | b',' if depth == 0 => break,
            b'-' if bytes.get(i + 1) == Some(&b'-') => skip_comment(source, &mut i, line),
            b'\n' => *line += 1,
            _ => {}
        }
        i += 1;
    }
    i
}

/// Parses `function name(params)` headers. Returns the bound function name
/// (only meaningful for `local function` / `const function`), the parameter
/// bindings, and the index just after the closing parenthesis.
fn parse_function_header(
    source: &str,
    index: usize,
    line: &mut usize,
) -> (Option<String>, Vec<RawBinding>, usize) {
    let bytes = source.as_bytes();
    let mut i = skip_whitespace(source, index, line);
    let mut name = None;
    let mut is_method = false;

    if is_identifier_start(bytes.get(i).copied()) {
        let start = i;
        i = read_identifier_end(source, i);
        name = Some(source[start..i].to_string());
        // `function Foo.bar()` / `function Foo:bar()` is a member assignment,
        // not a local declaration; the method form gets an implicit `self`.
        loop {
            let mut probe = skip_whitespace(source, i, line);
            let separator = match bytes.get(probe) {
                Some(&b'.') => Some('.'),
                Some(&b':') => Some(':'),
                _ => None,
            };
            let Some(separator) = separator else { break };
            if separator == ':' {
                is_method = true;
            }
            probe += 1;
            let member_start = skip_whitespace(source, probe, line);
            if is_identifier_start(bytes.get(member_start).copied()) {
                i = read_identifier_end(source, member_start);
                name = None;
            } else {
                break;
            }
        }
    }

    // Advance past optional generics (`function foo<T>(`) to the parameter list.
    while let Some(&byte) = bytes.get(i) {
        if byte == b'(' {
            break;
        }
        if byte == b'\n' {
            *line += 1;
        }
        i += 1;
    }

    let (mut params, after) = parse_parameters(source, i, line);
    if is_method {
        params.insert(
            0,
            RawBinding { name: "self".into(), annotation: None, instance_class: None, instance_path: None, line: *line },
        );
    }
    (name, params, after)
}

fn parse_parameters(source: &str, open: usize, line: &mut usize) -> (Vec<RawBinding>, usize) {
    let mut params = Vec::new();
    let bytes = source.as_bytes();
    let mut i = skip_whitespace(source, open + 1, line);
    let param_line = *line;
    while i < source.len() {
        i = skip_whitespace(source, i, line);
        let Some(&byte) = bytes.get(i) else { break };
        match byte {
            b')' => {
                i += 1;
                break;
            }
            b',' => i += 1,
            _ if is_identifier_start(Some(byte)) => {
                let start = i;
                i = read_identifier_end(source, i);
                let name = source[start..i].to_string();
                i = skip_whitespace(source, i, line);
                let mut annotation = None;
                if bytes.get(i) == Some(&b':') {
                    let (parsed, next) = skip_type_annotation(source, i + 1, line);
                    annotation = parsed;
                    i = next;
                    i = skip_whitespace(source, i, line);
                }
                if bytes.get(i) == Some(&b'=') {
                    i = skip_expression(source, i + 1, line);
                    i = skip_whitespace(source, i, line);
                }
                params.push(RawBinding { name, annotation, instance_class: None, instance_path: None, line: param_line });
            }
            _ => i += 1,
        }
    }
    (params, i)
}

/// The inferred type of an expression: a Roblox API class plus, when the
/// value has a statically known location, its DataModel path ("" is the
/// root). The path is what lets `game.ReplicatedStorage.ClientRenderRequest`
/// resolve to a RemoteEvent with no annotation.
#[derive(Debug, Clone, Default)]
struct InferredType {
    class: Option<String>,
    path: Option<String>,
}

impl InferredType {
    fn is_unknown(&self) -> bool {
        self.class.is_none() && self.path.is_none()
    }
}

/// Assigns an API class, tracking the DataModel path when the value is a
/// service (services live at the root under their class name), so
/// `local rs = game:GetService("ReplicatedStorage")` keeps resolving
/// `rs.SomeChild` afterwards.
fn set_api_class(inferred: &mut InferredType, class: &str) {
    inferred.path = if schema::class_is_service(class) {
        Some(class.to_string())
    } else {
        None
    };
    inferred.class = Some(class.to_string());
}

/// Whether `name` is a known Roblox class, datatype, or enum — usable as a
/// typed annotation or as the result of type inference. Combines the
/// generated API dump tables with the embedded rbx_reflection database.
fn api_type_known(name: &str) -> bool {
    api::is_api_type(name) || schema::class_exists(name)
}

/// Infers the Roblox class of a binding from its initializer
/// (`Instance.new("Part")`, `game:GetService("Players")`, ...).
fn instance_class_from_initializer(rest: &str) -> Option<String> {
    let trimmed = rest.trim_start();
    let lower = trimmed.to_ascii_lowercase();
    let call = [
        "instance.new(",
        "instance.create(",
        "game.getservice(",
        "game:getservice(",
        "game.findservice(",
        "game:findservice(",
    ]
    .iter()
    .find_map(|call| lower.starts_with(*call).then_some(call.len()))?;
    let inner = trimmed[call..].trim_start();
    let quote = inner.chars().next()?;
    if !matches!(quote, '"' | '\'') {
        return None;
    }
    let inner = &inner[quote.len_utf8()..];
    let end = inner.find(quote)?;
    let class = &inner[..end];
    api_type_known(class).then(|| class.to_string())
}

/// Flatten every binding currently pushed into the scope stack, innermost
/// first — used while an initializer is being lexed (the RHS may reference
/// locals declared earlier in the same scope).
fn visible_bindings(scopes: &[(ScopeKind, LocalScope)]) -> Vec<LocalBinding> {
    let mut visible = Vec::new();
    for (_, scope) in scopes.iter().rev() {
        visible.extend(scope.bindings.iter().cloned());
    }
    visible
}

fn skip_ascii_ws(bytes: &[u8], mut index: usize) -> usize {
    while bytes.get(index).is_some_and(|byte| byte.is_ascii_whitespace()) {
        index += 1;
    }
    index
}

fn read_ascii_ident(bytes: &[u8], mut index: usize) -> (&str, usize) {
    let start = index;
    while bytes.get(index).is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_') {
        index += 1;
    }
    let end = index;
    let text = std::str::from_utf8(&bytes[start..end]).unwrap_or("");
    (text, end)
}

/// Skips a balanced argument list starting at `(`; returns the index just
/// after the matching `)` (strings and nested brackets are handled).
fn skip_call_args(source: &str, mut index: usize) -> usize {
    let bytes = source.as_bytes();
    if bytes.get(index) != Some(&b'(') {
        return index;
    }
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    while let Some(&byte) = bytes.get(index) {
        if let Some(active) = quote {
            if byte == b'\\' {
                index += 2;
                continue;
            }
            if byte == active {
                quote = None;
            }
            index += 1;
            continue;
        }
        match byte {
            b'"' | b'\'' | b'`' => quote = Some(byte),
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' if depth > 1 => depth -= 1,
            b')' if depth == 1 => return index + 1,
            _ => {}
        }
        index += 1;
    }
    index
}

/// First quoted argument of a call expression (`("Players")` → `Players`).
fn first_quoted_call_arg(expr: &str) -> Option<&str> {
    let bytes = expr.as_bytes();
    let index = skip_ascii_ws(bytes, 0);
    if bytes.get(index) != Some(&b'(') {
        return None;
    }
    let mut quote_index = index + 1;
    while let Some(&byte) = bytes.get(quote_index) {
        if byte == b'"' || byte == b'\'' {
            break;
        }
        if byte == b')' {
            return None;
        }
        quote_index += 1;
    }
    let quote = *bytes.get(quote_index)?;
    let start = quote_index + 1;
    let mut end = start;
    while let Some(&byte) = bytes.get(end) {
        if byte == quote {
            let value = &expr[start..end];
            return (!value.is_empty()).then_some(value);
        }
        end += 1;
    }
    None
}

/// The initializer expression after `=`, up to a top-level `,`/`;`/newline
/// (strings, brackets, and nested calls are respected).
fn expression_fragment(source: &str, mut index: usize) -> &str {
    let bytes = source.as_bytes();
    index = skip_ascii_ws(bytes, index);
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    while let Some(&byte) = bytes.get(index) {
        if let Some(active) = quote {
            if byte == b'\\' {
                index += 2;
                continue;
            }
            if byte == active {
                quote = None;
            }
            index += 1;
            continue;
        }
        match byte {
            b'"' | b'\'' | b'`' => quote = Some(byte),
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' if depth > 0 => depth -= 1,
            b'\n' | b';' if depth == 0 => break,
            b',' if depth == 0 => break,
            _ => {}
        }
        index += 1;
    }
    &source[..index]
}

/// Infers the API type of an initializer: identifier, member chain
/// (`Actor.Player`), or service call (`game:GetService("Players")`), using
/// already-visible local bindings and the generated API tables.
fn infer_expression_type(
    index: &ProjectIndex,
    rest: &str,
    locals: &[LocalBinding],
    current_script: Ref,
) -> InferredType {
    let fragment = expression_fragment(rest, 0);
    let bytes = fragment.as_bytes();
    let mut i = skip_ascii_ws(bytes, 0);
    let (base, after) = read_ascii_ident(bytes, i);
    if base.is_empty() {
        return InferredType::default();
    }
    i = after;

    let mut inferred = InferredType::default();
    if let Some(binding) = locals.iter().find(|binding| binding.name == base) {
        // A local shadows the built-in even when its type is unknown.
        inferred.class = binding.instance_class.clone();
        inferred.path = binding.instance_path.clone();
    } else if let Some(api_type) = api::global_type(base) {
        if api_type.starts_with("@lib:") || api_type == "@enum" {
            return InferredType::default();
        }
        let path = index.global_path(base, api_type, current_script);
        // `script` takes the class of the script being edited.
        let class = if base == "script" {
            path.as_deref()
                .and_then(|script_path| index.dom_class_at_path(script_path))
                .unwrap_or(api_type)
        } else {
            api_type
        };
        inferred.class = Some(class.to_string());
        inferred.path = path;
    } else {
        return InferredType::default();
    }
    if inferred.is_unknown() {
        return inferred;
    }

    loop {
        i = skip_ascii_ws(bytes, i);
        let _separator = match bytes.get(i) {
            Some(b'.') => '.',
            Some(b':') => ':',
            _ => break,
        };
        i += 1;
        i = skip_ascii_ws(bytes, i);
        let (member, after) = read_ascii_ident(bytes, i);
        if member.is_empty() {
            return InferredType::default();
        }
        i = after;
        i = skip_ascii_ws(bytes, i);
        let call_start = i;
        let is_call = bytes.get(i) == Some(&b'(');
        if is_call {
            i = skip_call_args(fragment, i);
        }

        if !infer_step(index, &mut inferred, member, is_call, &fragment[call_start..]) {
            return InferredType::default();
        }
    }
    inferred
}

/// Advances an in-progress inference across one `.member` / `:member` step
/// (plus the step's call arguments, if any). A value with a DataModel
/// location resolves children exactly; otherwise the API tables apply.
/// Returns false when the step cannot be resolved at all.
fn infer_step(
    index: &ProjectIndex,
    inferred: &mut InferredType,
    member: &str,
    is_call: bool,
    call_args: &str,
) -> bool {
    // A value with a DataModel location resolves children exactly.
    if let Some(path) = inferred.path.clone() {
        // `x.Parent` climbs the tree.
        if member == "Parent" && !is_call {
            if !path.is_empty() {
                let parent = path
                    .rsplit_once('/')
                    .map(|(head, _)| head.to_string())
                    .unwrap_or_default();
                inferred.path = Some(parent.clone());
                inferred.class = Some(
                    index
                        .dom_class_at_path(&parent)
                        .unwrap_or("Instance")
                        .to_string(),
                );
                return true;
            }
        }
        // `x:WaitForChild("Name")` / `x:FindFirstChild("Name")` with a
        // literal resolves the real child's class.
        if is_call && matches!(member, "WaitForChild" | "FindFirstChild") {
            if let Some(child_name) = first_quoted_call_arg(call_args) {
                if let Some((child_path, child_class)) = index.dom_child(&path, child_name) {
                    inferred.path = Some(child_path);
                    inferred.class = Some(child_class);
                    return true;
                }
            }
            set_api_class(inferred, "Instance");
            return true;
        }
        // `x:FindFirstAncestor("Name")` climbs by name.
        if is_call && member == "FindFirstAncestor" {
            if let Some(want) = first_quoted_call_arg(call_args) {
                let mut probe = path
                    .rsplit_once('/')
                    .map(|(head, _)| head)
                    .unwrap_or("");
                loop {
                    let name = probe.rsplit('/').next().unwrap_or("");
                    if name == want {
                        inferred.path = Some(probe.to_string());
                        inferred.class = Some(
                            index
                                .dom_class_at_path(probe)
                                .unwrap_or("Instance")
                                .to_string(),
                        );
                        return true;
                    }
                    if probe.is_empty() {
                        break;
                    }
                    probe = probe.rsplit_once('/').map(|(head, _)| head).unwrap_or("");
                }
            }
            set_api_class(inferred, "Instance");
            return true;
        }
        // Plain `.ChildName` access prefers the real DataModel child.
        if !is_call {
            if let Some((child_path, child_class)) = index.dom_child(&path, member) {
                inferred.path = Some(child_path);
                inferred.class = Some(child_class);
                return true;
            }
        }
    }

    // `x:FindFirstChildOfClass("RemoteEvent")` -> the class itself.
    if is_call
        && matches!(
            member,
            "FindFirstChildOfClass"
                | "FindFirstAncestorOfClass"
                | "FindFirstChildWhichIsA"
                | "FindFirstAncestorWhichIsA"
        )
    {
        if let Some(class) = first_quoted_call_arg(call_args).filter(|class| api_type_known(class)) {
            set_api_class(inferred, class);
            return true;
        }
    }
    // `game:GetService("Players")` / `game:FindService(...)` — the quoted
    // argument beats the generic Instance return type, and services keep
    // their root path so their children keep resolving.
    if matches!(member, "GetService" | "FindService") {
        if let Some(class) = first_quoted_call_arg(call_args).filter(|class| api_type_known(class)) {
            set_api_class(inferred, class);
            return true;
        }
    }
    // `Instance.new("Part")` / `Instance.create("Part")` — the quoted
    // class argument is the result type.
    if is_call && matches!(member, "new" | "create") && inferred.class.as_deref() == Some("Instance")
    {
        if let Some(class) = first_quoted_call_arg(call_args).filter(|class| api_type_known(class)) {
            set_api_class(inferred, class);
            return true;
        }
    }
    // Unresolvable searches stay a generic Instance.
    if is_call
        && matches!(
            member,
            "FindFirstChild" | "WaitForChild" | "FindFirstChildOfClass" | "FindFirstAncestor"
        )
    {
        set_api_class(inferred, "Instance");
        return true;
    }
    // Datatype constructors (`Vector3.new(...)`, `CFrame.new(...)`, ...)
    // return the datatype itself.
    if is_call
        && member == "new"
        && inferred
            .class
            .as_deref()
            .is_some_and(|class| api::find_class(class).is_some_and(|found| found.datatype))
    {
        return true;
    }
    // Anything else flows through the API member types (dropping the path).
    if let Some(class) = inferred.class.clone() {
        if let Some(next) = api::member_type(&class, member) {
            set_api_class(inferred, next);
            return true;
        }
    }
    false
}

/// Best-effort class for `local x = <initializer>`. The generic chain
/// inference runs first because it handles `game:GetService("Players")
/// .LocalPlayer` (→ Player) where the special-cased helper would stop at
/// the service (`Players`).
fn infer_binding_type(
    index: &ProjectIndex,
    rest: &str,
    locals: &[LocalBinding],
    current_script: Ref,
) -> InferredType {
    let inferred = infer_expression_type(index, rest, locals, current_script);
    if inferred.is_unknown() {
        if let Some(class) = instance_class_from_initializer(rest) {
            let mut fallback = InferredType::default();
            set_api_class(&mut fallback, &class);
            return fallback;
        }
    }
    inferred
}

fn commit_bindings(scopes: &mut [(ScopeKind, LocalScope)], names: &mut Vec<RawBinding>, is_const: bool) {
    if names.is_empty() {
        return;
    }
    let base = if is_const { "const variable" } else { "local variable" };
    let target = &mut scopes.last_mut().unwrap().1;
    for binding in names.drain(..) {
        let instance_class = binding
            .instance_class
            .or_else(|| binding.annotation.clone().filter(|class| api_type_known(class)));
        let detail = instance_class
            .as_ref()
            .map_or_else(|| base.to_string(), |class| format!("{base} · {class}"));
        target.bindings.push(LocalBinding {
            name: binding.name,
            detail,
            line: binding.line,
            instance_class,
            instance_path: binding.instance_path,
        });
    }
}

/// Locals that are actually in scope at the caret, walking Luau block scopes
/// (`function`/`if`/`for`/`while`/`do`/`repeat`). Bindings are returned
/// innermost-first so a shadowing binding wins over an outer one.
fn local_bindings_at(
    index: &ProjectIndex,
    source: &str,
    cursor_char: usize,
    current_script: Ref,
) -> Vec<LocalBinding> {
    let cursor_byte = char_to_byte(source, cursor_char);
    let source = &source[..cursor_byte];

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum DeclState {
        Seen { is_const: bool },
        Names { is_const: bool },
    }

    let mut scopes: Vec<(ScopeKind, LocalScope)> = vec![(ScopeKind::File, LocalScope::default())];
    let mut cursor = 0usize;
    let mut line = 1usize;
    let mut decl: Option<DeclState> = None;
    let mut names: Vec<RawBinding> = Vec::new();
    let mut expect_name = false;
    let mut for_vars: Vec<RawBinding> = Vec::new();
    /// None = not in a for header; Some(false) = headers still collecting the
    /// loop variables; Some(true) = variables collected, awaiting `do`.
    let mut in_for_header: Option<bool> = None;

    while let Some(token) = next_token(source, &mut cursor, &mut line) {
        match token {
            Token::Newline => {
                // `local x` without `=` is still a declaration once the
                // statement ends.
                if let Some(DeclState::Names { is_const }) = decl.take() {
                    commit_bindings(&mut scopes, &mut names, is_const);
                }
                expect_name = false;
            }
            Token::Word(word) => match word.to_ascii_lowercase().as_str() {
                "local" | "const" => {
                    if let Some(DeclState::Names { is_const }) = decl.take() {
                        commit_bindings(&mut scopes, &mut names, is_const);
                    }
                    decl = Some(DeclState::Seen { is_const: word.eq_ignore_ascii_case("const") });
                    expect_name = false;
                }
                "function" => {
                    let mut local_function = None;
                    if let Some(state) = decl.take() {
                        match state {
                            DeclState::Seen { is_const } => local_function = Some(is_const),
                            DeclState::Names { is_const } => {
                                commit_bindings(&mut scopes, &mut names, is_const);
                            }
                        }
                    }
                    let (name, params, next_index) = parse_function_header(source, cursor, &mut line);
                    cursor = next_index;
                    // Only `local function` / `const function` create a local
                    // binding. A bare `function Foo()` is a global, which must
                    // not be offered as an in-scope local.
                    if let (Some(is_const), Some(name)) = (local_function, name.as_deref()) {
                        let detail = if is_const { "const function" } else { "local function" };
                        scopes.last_mut().unwrap().1.bindings.push(LocalBinding {
                            name: name.to_string(),
                            detail: detail.into(),
                            line,
                            instance_class: None,
                            instance_path: None,
                        });
                    }
                    let mut function_scope = LocalScope::default();
                    for param in params {
                        let instance_class = param
                            .annotation
                            .clone()
                            .filter(|class| api_type_known(class));
                        let detail = instance_class.as_ref().map_or_else(
                            || "parameter".to_string(),
                            |class| format!("parameter · {class}"),
                        );
                        function_scope.bindings.push(LocalBinding {
                            name: param.name,
                            detail,
                            line: param.line,
                            instance_class,
                            instance_path: None,
                        });
                    }
                    scopes.push((ScopeKind::Function, function_scope));
                }
                "for" => {
                    in_for_header = Some(false);
                    for_vars.clear();
                    expect_name = false;
                }
                "in" => {
                    if in_for_header.is_some() {
                        in_for_header = Some(true);
                    }
                }
                "do" => {
                    if in_for_header.is_some() {
                        let mut loop_scope = LocalScope::default();
                        for var in for_vars.drain(..) {
                            loop_scope.bindings.push(LocalBinding {
                                name: var.name,
                                detail: "loop variable".into(),
                                line: var.line,
                                instance_class: None,
                                instance_path: None,
                            });
                        }
                        scopes.push((ScopeKind::Loop, loop_scope));
                        in_for_header = None;
                    } else {
                        scopes.push((ScopeKind::Block, LocalScope::default()));
                    }
                }
                "then" => scopes.push((ScopeKind::IfBranch, LocalScope::default())),
                "elseif" => {
                    if scopes.last().is_some_and(|(kind, _)| *kind == ScopeKind::IfBranch) {
                        scopes.pop();
                    }
                }
                "else" => {
                    // The else-branch has its own scope: replace the previous
                    // branch scope so its locals die at the matching `end`.
                    if scopes.last().is_some_and(|(kind, _)| *kind == ScopeKind::IfBranch) {
                        scopes.pop();
                        scopes.push((ScopeKind::IfBranch, LocalScope::default()));
                    }
                }
                "repeat" => scopes.push((ScopeKind::Repeat, LocalScope::default())),
                "until" => {
                    if scopes.last().is_some_and(|(kind, _)| *kind == ScopeKind::Repeat) {
                        scopes.pop();
                    }
                }
                "end" => {
                    if scopes.len() > 1 {
                        scopes.pop();
                    }
                }
                _ => {
                    if let Some(state) = decl {
                        match state {
                            DeclState::Seen { is_const } => {
                                names.push(RawBinding {
                                    name: word.to_string(),
                                    annotation: None,
                                    instance_class: None,
                                    instance_path: None,
                                    line,
                                });
                                decl = Some(DeclState::Names { is_const });
                            }
                            DeclState::Names { .. } if expect_name => {
                                names.push(RawBinding {
                                    name: word.to_string(),
                                    annotation: None,
                                    instance_class: None,
                                    instance_path: None,
                                    line,
                                });
                                expect_name = false;
                            }
                            DeclState::Names { .. } => {}
                        }
                    } else if matches!(in_for_header, Some(false)) {
                        for_vars.push(RawBinding {
                            name: word.to_string(),
                            annotation: None,
                            instance_class: None,
                            instance_path: None,
                            line,
                        });
                    }
                }
            },
            Token::Op(operator) => match operator {
                "," => {
                    if matches!(decl, Some(DeclState::Names { .. })) {
                        expect_name = true;
                    }
                }
                "=" => {
                    if let Some(DeclState::Names { is_const }) = decl.take() {
                        let inferred = infer_binding_type(
                            index,
                            &source[cursor..],
                            &visible_bindings(&scopes),
                            current_script,
                        );
                        if let Some(last) = names.last_mut() {
                            last.instance_class = inferred.class;
                            last.instance_path = inferred.path;
                        }
                        commit_bindings(&mut scopes, &mut names, is_const);
                    }
                    if matches!(in_for_header, Some(false)) {
                        in_for_header = Some(true);
                    }
                    expect_name = false;
                }
                ";" => {
                    if let Some(DeclState::Names { is_const }) = decl.take() {
                        commit_bindings(&mut scopes, &mut names, is_const);
                    }
                    expect_name = false;
                }
                ":" => {
                    // `local name: Type` — the type must not be scanned for
                    // more names, so skip it and remember the class name.
                    if let Some(DeclState::Names { .. }) = decl {
                        if !expect_name && names.last().is_some() {
                            let (annotation, next) = skip_type_annotation(source, cursor, &mut line);
                            cursor = next;
                            if let Some(annotation) = annotation {
                                if let Some(last) = names.last_mut() {
                                    last.annotation = Some(annotation);
                                }
                            }
                        }
                    }
                }
                _ => {}
            },
        }
    }

    // Pending names are deliberately NOT committed at EOF: while the caret is
    // still inside `local pa|`, the in-progress name must not shadow existing
    // locals (and must not be offered as a completion of itself).
    visible_bindings(&scopes)
}

fn innermost_local<'a>(locals: &'a [LocalBinding], name: &str) -> Option<&'a LocalBinding> {
    locals.iter().find(|binding| binding.name == name)
}

fn constructor_module_alias<'a>(source: &'a str, variable: &str) -> Option<&'a str> {
    source.lines().rev().find_map(|raw| {
        let code = raw.split("--").next().unwrap_or("").trim();
        let declaration = code.strip_prefix("local ").or_else(|| code.strip_prefix("const "))?;
        let (left, right) = declaration.split_once('=')?;
        let name = left.split(':').next().unwrap_or(left).trim();
        if name != variable { return None; }
        let call = right.trim();
        let dot = call.find('.')?;
        let module_alias = call[..dot].trim();
        let method = identifier_start(&call[dot + 1..]);
        (is_identifier(module_alias) && matches!(method, "new" | "create" | "Create"))
            .then_some(module_alias)
    })
}

fn nested_member_expression_at_end(source: &str) -> Option<(&str, String, &str, char)> {
    let tail = source.trim_end_matches(char::is_whitespace)
        .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || matches!(c, '.' | ':'))).next()?;
    let last_separator = tail.rfind(|c| c == '.' || c == ':')?;
    let separator = tail.as_bytes()[last_separator] as char;
    let parts: Vec<&str> = tail.split(|c| c == '.' || c == ':').collect();
    if parts.len() < 3 || !is_identifier(parts[0]) { return None; }
    if !parts[1..parts.len() - 1].iter().all(|part| is_identifier(part)) { return None; }
    let prefix = parts.last().copied().unwrap_or("");
    if !prefix.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') { return None; }
    Some((parts[0], parts[1..parts.len() - 1].join("."), prefix, separator))
}

fn member_expression_at_end(source: &str) -> Option<(&str, &str, char)> {
    let tail = source.trim_end_matches(char::is_whitespace)
        .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || matches!(c, '.' | ':'))).next()?;
    let dot = tail.rfind('.');
    let colon = tail.rfind(':');
    let (position, separator) = match (dot, colon) {
        (Some(a), Some(b)) if b > a => (b, ':'),
        (Some(a), _) => (a, '.'),
        (_, Some(b)) => (b, ':'),
        _ => return None,
    };
    let alias = &tail[..position];
    let prefix = &tail[position + 1..];
    (is_identifier(alias) && prefix.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .then_some((alias, prefix, separator))
}

fn member_matches_access(detail: &str, separator: char) -> bool {
    let method = detail.starts_with("method ");
    if separator == ':' { method } else { !method }
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

/// Infer the public shape of objects produced by the common Roblox/Luau OOP
/// pattern (`Class.__index = Class`, `Class.new`, `setmetatable`, and `self.x`).
fn constructed_object_members(source: &str) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    let mut classes = std::collections::BTreeSet::new();

    // Colon methods are instance methods. Dot methods other than constructors
    // are included too because many modules use `Class.Destroy(self)` style.
    for raw in source.lines() {
        let code = raw.split("--").next().unwrap_or("").trim();
        let Some(declaration) = code.strip_prefix("function ")
            .or_else(|| code.strip_prefix("local function ")) else { continue };
        if let Some((owner, rest)) = declaration.split_once(':') {
            let member = identifier_start(rest);
            if is_identifier(owner.trim()) && is_identifier(member) {
                classes.insert(owner.trim().to_string());
                let signature = function_signature(member, &rest[member.len()..])
                    .unwrap_or_else(|| format!("function {member}"));
                result.insert(member.into(), format!("method {signature}"));
            }
        } else if let Some((owner, rest)) = declaration.split_once('.') {
            let member = identifier_start(rest);
            if is_identifier(owner.trim()) && is_identifier(member) {
                classes.insert(owner.trim().to_string());
                if !matches!(member, "new" | "create" | "Create") {
                    result.insert(member.into(), function_signature(member, &rest[member.len()..])
                        .unwrap_or_else(|| format!("method {member}")));
                }
            }
        }
    }

    // Fields assigned to the constructed receiver are part of its shape.
    // Accept conventional names (`self`) and variables initialized through
    // setmetatable, including a preset/default table.
    let mut receivers: std::collections::HashSet<String> = ["self".to_string()].into_iter().collect();
    let mut preset_names = std::collections::BTreeSet::new();
    for raw in source.lines() {
        let code = raw.split("--").next().unwrap_or("").trim();
        if code.contains("setmetatable(") {
            if let Some((left, right)) = code.split_once('=') {
                let name = left.trim().trim_start_matches("local ").split(':').next().unwrap_or("").trim();
                if is_identifier(name) { receivers.insert(name.into()); }
                for token in right.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
                    if is_identifier(token) && !classes.contains(token)
                        && !matches!(token, "setmetatable" | "table" | "clone")
                    { preset_names.insert(token.to_string()); }
                }
            }
        }
        if let Some((left, _)) = code.split_once('=') {
            if let Some((receiver, field)) = left.trim().split_once('.') {
                let field = field.trim();
                if receivers.contains(receiver.trim()) && is_identifier(field) {
                    result.entry(field.into()).or_insert_with(|| format!("instance field {field}"));
                }
            }
        }
    }

    // Read direct keys from preset tables used by setmetatable/table.clone.
    for preset in preset_names {
        let mut in_table = false;
        let mut depth = 0i32;
        for raw in source.lines() {
            let code = raw.split("--").next().unwrap_or("").trim();
            if !in_table {
                let declaration = code.strip_prefix("local ").or_else(|| code.strip_prefix("const "));
                if declaration.is_some_and(|value| value.starts_with(&format!("{preset} = {{"))) {
                    in_table = true;
                    depth = structural_brace_delta(code);
                }
                continue;
            }
            if depth == 1 {
                let field = identifier_start(code);
                if is_identifier(field) && code[field.len()..].trim_start().starts_with('=') {
                    result.entry(field.into()).or_insert_with(|| format!("preset field {field}"));
                }
            }
            depth += structural_brace_delta(code);
            if depth <= 0 { break; }
        }
    }
    result
}

fn returned_member_paths(source: &str) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    let mut depth: Option<i32> = None;
    let mut parents: HashMap<i32, String> = HashMap::new();
    for raw in source.lines() {
        let code = raw.split("--").next().unwrap_or("").trim();
        let starts = depth.is_none() && code.starts_with("return {");
        if starts { depth = Some(1); parents.insert(1, String::new()); }
        let Some(current) = depth else { continue };
        let body = code.strip_prefix("return {").unwrap_or(code).trim();
        let name = identifier_start(body);
        if is_identifier(name) && body[name.len()..].trim_start().starts_with('=') {
            let parent = parents.get(&current).cloned().unwrap_or_default();
            let path = if parent.is_empty() { name.to_string() } else { format!("{parent}.{name}") };
            let rhs = body.split_once('=').map_or("", |(_, rhs)| rhs.trim());
            result.insert(path.clone(), if rhs.starts_with('{') { format!("table {name}") } else { format!("field {name}") });
            if rhs.starts_with('{') { parents.insert(current + 1, path); }
        } else if body.starts_with('[') {
            let rhs = body.split_once('=').map_or("", |(_, rhs)| rhs.trim());
            if rhs.starts_with('{') {
                parents.insert(current + 1, parents.get(&current).cloned().unwrap_or_default());
            }
        }
        let delta = structural_brace_delta(code);
        let next = if starts { delta } else { current + delta };
        if next <= 0 { depth = None; parents.clear(); } else {
            depth = Some(next);
            parents.retain(|level, _| *level <= next);
        }
    }
    result
}

fn exported_members(source: &str) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    // Brace depth inside the table returned by the module. Only fields at
    // depth one are public module members; nested configuration keys are not.
    let mut return_depth: Option<i32> = None;
    for line in source.lines() {
        let code = line.split("--").next().unwrap_or("").trim();
        // Only a real owner-qualified function declaration exports a method.
        // Previously every dotted expression was accepted, so a value such as
        // `Color = Color3.fromRGB(...)` incorrectly exported `fromRGB` as a
        // ModuleScript table function.
        let method_declaration = code.strip_prefix("function ")
            .or_else(|| code.strip_prefix("const function "));
        if let Some(declaration) = method_declaration {
            // Determine the separator based on the last '.' or ':' that
            // appears *before* the parameter list — not anywhere in the
            // signature. Parameter type annotations (e.g.
            // "Data: DataSettings") also contain ':' and were previously
            // misread as a method-call separator, so
            // `function Module.Ragdoll(Data: DataSettings)` was wrongly
            // classified as a `:` method instead of a `.` function.
            let head = declaration.split('(').next().unwrap_or(declaration);
            let colon_method = match (head.rfind('.'), head.rfind(':')) {
                (Some(dot), Some(colon)) => colon > dot,
                (None, Some(_)) => true,
                _ => false,
            };
            if let Some((owner, member)) = declaration.split_once('.')
                .or_else(|| declaration.split_once(':'))
            {
                let name = identifier_start(member);
                if is_identifier(owner.trim()) && is_identifier(name) {
                    let suffix = &member[name.len()..];
                    let signature = function_signature(name, suffix)
                        .unwrap_or_else(|| format!("function {name}"));
                    result.insert(name.to_string(), if colon_method {
                        format!("method {signature}")
                    } else { signature });
                }
            }
        } else if let Some((lhs, rhs)) = code.split_once('=') {
            // Also support `Module.Method = function(...)` exports without
            // mistaking function calls on the right-hand side for exports.
            if rhs.trim_start().starts_with("function") {
                if let Some((owner, member)) = lhs.trim().split_once('.') {
                    let name = member.trim();
                    if is_identifier(owner.trim()) && is_identifier(name) {
                        result.insert(name.to_string(), format!("function {name}"));
                    }
                }
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
        // Also understand `return { foo = value }`, but do not leak keys from
        // nested tables (Moves.Base[1].KeyBind) into `Module.KeyBind`.
        let starts_return = return_depth.is_none() && code.starts_with("return {");
        if starts_return {
            return_depth = Some(1);
        }
        if return_depth == Some(1) {
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
        }
        if let Some(depth) = return_depth {
            let delta = structural_brace_delta(code);
            let next = if starts_return { delta } else { depth + delta };
            return_depth = (next > 0).then_some(next);
        }
    }
    // Modules very often build their public API in a named table and then
    // `return TheTable` at the very end — frequently wrapped in
    // `table.freeze(setmetatable({ ... }, mt)) :: any` and typed with an
    // `export type` interface. None of that is a literal `return {`, so the
    // scanning above saw no exports at all. Recover those members from the
    // returned binding, and enrich them with the annotated interface.
    for (name, detail) in returned_binding_members(source) {
        result.entry(name).or_insert(detail);
    }
    result
}

/// Name of the identifier the module returns (`return Foo`, `return Foo :: any`).
fn returned_binding_name(source: &str) -> Option<&str> {
    source.lines().rev().find_map(|raw| {
        let code = raw.split("--").next().unwrap_or("").trim();
        let rest = code.strip_prefix("return ")?;
        let mut rest = rest.split("::").next().unwrap_or(rest).trim();
        // Unwrap the usual export wrappers: `return table.freeze(Module)`,
        // `return setmetatable(Module, mt)`, `return (Module)`.
        loop {
            let unwrapped = ["table.freeze(", "table.clone(", "setmetatable(", "("]
                .into_iter()
                .find_map(|prefix| rest.strip_prefix(prefix));
            let Some(inner) = unwrapped else { break };
            rest = inner.split(',').next().unwrap_or(inner).trim_end_matches(')').trim();
        }
        is_identifier(rest).then_some(rest)
    })
}

/// Locate `local/const Name[: Type] = ...`, returning the annotation (if any)
/// and the zero-based line index of the declaration.
fn binding_declaration<'a>(source: &'a str, name: &str) -> Option<(Option<&'a str>, usize)> {
    source.lines().enumerate().find_map(|(index, raw)| {
        let code = raw.split("--").next().unwrap_or("").trim();
        let rest = code.strip_prefix("local ")
            .or_else(|| code.strip_prefix("const "))
            .unwrap_or(code);
        let rest = rest.strip_prefix(name)?;
        // `Registry` must not match the declaration of `RegistryMetatable`.
        if rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') { return None }
        let rest = rest.trim_start();
        let (annotation, rest) = match rest.strip_prefix(':') {
            Some(tail) => {
                let (annotation, value) = tail.split_once('=')?;
                (Some(annotation.trim()), value)
            }
            None => (None, rest.strip_prefix('=')?),
        };
        // The value itself is parsed separately by table_literal_members.
        let _value = rest;
        Some((annotation, index))
    })
}

/// Public members of the returned table, taken from the table literal at the
/// declaration and from its annotated `export type` interface (which carries
/// the richer function signatures).
fn returned_binding_members(source: &str) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    let Some(name) = returned_binding_name(source) else { return result };
    let Some((annotation, line)) = binding_declaration(source, name) else { return result };
    for (member, detail) in table_literal_members(source, line) {
        result.insert(member, detail);
    }
    // Members attached after the declaration: `Module.Value = 5`,
    // `Module.Client = {}`, `Module.Nested.Thing = ...` (Knit/Fusion style).
    let owner_prefix = format!("{name}.");
    for raw in source.lines() {
        let code = raw.split("--").next().unwrap_or("").trim();
        let code = code.strip_prefix("function ")
            .or_else(|| code.strip_prefix("local function "))
            .or_else(|| code.strip_prefix("const function "))
            .unwrap_or(code);
        let Some(rest) = code.strip_prefix(&owner_prefix) else { continue };
        let member = identifier_start(rest);
        if !is_identifier(member) || is_metamethod(member) { continue }
        let tail = rest[member.len()..].trim_start();
        let detail = if tail.starts_with('(') {
            function_signature(member, &rest[member.len()..])
                .unwrap_or_else(|| format!("function {member}"))
        } else if let Some(value) = tail.strip_prefix('=') {
            if value.trim_start().starts_with("function") {
                format!("function {member}")
            } else {
                format!("field {member}")
            }
        } else {
            continue;
        };
        result.insert(member.to_string(), detail);
    }
    if let Some(annotation) = annotation {
        // A `Registry` style interface documents the real signatures, so it
        // wins over the bare `field X` inferred from the literal.
        let type_name = identifier_start(annotation);
        for (member, detail) in type_declaration_members(source, type_name) {
            result.insert(member, detail);
        }
    }
    result
}

/// Fields of the first table literal starting at (or just after) `start_line`,
/// ignoring nested tables. Wrappers such as `table.freeze(setmetatable({` are
/// skipped because only the brace depth is tracked.
fn table_literal_members(source: &str, start_line: usize) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    let mut depth = 0i32;
    let mut started = false;
    for raw in source.lines().skip(start_line) {
        let code = raw.split("--").next().unwrap_or("").trim();
        if !started {
            let Some(open) = code.find('{') else {
                // Only keep looking while the declaration is still an open
                // call such as `table.freeze(`; otherwise this binding is not
                // a table literal and an unrelated table must not be adopted.
                if structural_paren_delta(code) > 0 { continue } else { break }
            };
            started = true;
            depth = structural_brace_delta(code);
            if depth == 1 {
                collect_table_fields(&code[open + 1..], &mut result);
            }
            if depth <= 0 { break }
            continue;
        }
        if depth == 1 {
            collect_table_fields(code, &mut result);
        }
        depth += structural_brace_delta(code);
        if depth <= 0 { break }
    }
    result
}

/// Members declared by `type Name = { ... }` / `export type Name = { ... }`.
fn type_declaration_members(source: &str, type_name: &str) -> BTreeMap<String, String> {
    for (index, raw) in source.lines().enumerate() {
        let code = raw.split("--").next().unwrap_or("").trim();
        let rest = code.strip_prefix("export type ").or_else(|| code.strip_prefix("type "));
        let Some(rest) = rest else { continue };
        if identifier_start(rest) == type_name && code.contains('{') {
            return table_literal_members(source, index);
        }
    }
    BTreeMap::new()
}

/// Luau metamethods (`__index`, `__tostring`, ...) are implementation detail
/// of the metatable, never a member a user means to autocomplete.
fn is_metamethod(name: &str) -> bool {
    name.starts_with("__")
}

/// Whether a module's public surface cannot be fully determined by reading
/// the source: computed keys, forwarded modules, callable modules, or an
/// `__index` fallback to another table. Completion still offers whatever was
/// found, but "unknown member" diagnostics must stay silent for these.
pub(crate) fn has_dynamic_exports(source: &str) -> bool {
    let returned = returned_binding_name(source);
    for raw in source.lines() {
        let code = raw.split("--").next().unwrap_or("").trim();
        // `return require(...)` / `return function(...)` — no member table.
        if let Some(rest) = code.strip_prefix("return ") {
            let rest = rest.split("::").next().unwrap_or(rest).trim();
            if rest.starts_with("require(") || rest.starts_with("function") {
                return true;
            }
        }
        let Some(name) = returned else { continue };
        // `M[key] = ...` builds exports with a computed key.
        if code.starts_with(&format!("{name}[")) {
            return true;
        }
        // `M.__index = Other` forwards lookups to a table we did not scan.
        if let Some(rest) = code.strip_prefix(&format!("{name}.__index")) {
            let target = rest.trim_start().strip_prefix('=').map(str::trim).unwrap_or("");
            if is_identifier(target) && target != name {
                return true;
            }
        }
        if code.contains("setmetatable(") && code.contains("__index") {
            return true;
        }
    }
    false
}

fn collect_table_fields(body: &str, result: &mut BTreeMap<String, String>) {
    for field in split_top_level_fields(body) {
        let field = field.trim().trim_end_matches([',', ';']).trim();
        let name = identifier_start(field);
        if !is_identifier(name) || is_metamethod(name) { continue }
        let rest = field[name.len()..].trim_start();
        if let Some(value) = rest.strip_prefix('=') {
            let value = value.trim();
            let detail = if let Some(tail) = value.strip_prefix("function") {
                function_signature(name, tail).unwrap_or_else(|| format!("function {name}"))
            } else {
                format!("field {name}")
            };
            result.entry(name.to_string()).or_insert(detail);
        } else if let Some(annotation) = rest.strip_prefix(':') {
            // Interface entry: `Get: (target: StateTarget) -> StateProxy?`
            let annotation = annotation.trim();
            if annotation.is_empty() { continue }
            let detail = if annotation.contains("->") {
                format!("function {name}{annotation}")
            } else {
                format!("field {name}: {annotation}")
            };
            result.entry(name.to_string()).or_insert(detail);
        }
    }
}

/// Split a table body on commas/semicolons that are not nested inside
/// brackets, braces, parentheses, or string literals.
fn split_top_level_fields(body: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for character in body.chars() {
        if escaped { current.push(character); escaped = false; continue }
        if let Some(active) = quote {
            current.push(character);
            if character == '\\' { escaped = true; }
            else if character == active { quote = None; }
            continue;
        }
        match character {
            '\'' | '"' | '`' => { quote = Some(character); current.push(character); }
            '{' | '(' | '[' => { depth += 1; current.push(character); }
            '}' | ')' | ']' => { depth -= 1; current.push(character); }
            ',' | ';' if depth <= 0 => { result.push(std::mem::take(&mut current)); }
            _ => current.push(character),
        }
    }
    result.push(current);
    result
}

/// Net parenthesis balance outside string literals.
fn structural_paren_delta(code: &str) -> i32 {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut delta = 0;
    for character in code.chars() {
        if escaped { escaped = false; continue }
        if let Some(active) = quote {
            if character == '\\' { escaped = true; }
            else if character == active { quote = None; }
            continue;
        }
        match character {
            '\'' | '"' | '`' => quote = Some(character),
            '(' => delta += 1,
            ')' => delta -= 1,
            _ => {}
        }
    }
    delta
}

fn structural_brace_delta(code: &str) -> i32 {
    let mut quote = None;
    let mut escaped = false;
    let mut delta = 0;
    for character in code.chars() {
        if escaped { escaped = false; continue; }
        if character == '\\' && quote.is_some() { escaped = true; continue; }
        if let Some(active) = quote {
            if character == active { quote = None; }
            continue;
        }
        if matches!(character, '\'' | '"' | '`') { quote = Some(character); continue; }
        if character == '{' { delta += 1; }
        if character == '}' { delta -= 1; }
    }
    delta
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
    use rbx_dom_weak::InstanceBuilder;

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
        let suggestions = index.complete_at(Ref::none(), "game:F", 6);
        assert!(suggestions.iter().any(|item| item.label == "FindService"));
        assert!(!suggestions.iter().any(|item| item.label == "game"));
    }

    #[test]
    fn nested_member_chains_never_fall_back_to_keywords() {
        let index = ProjectIndex::default();
        assert!(index.complete_at(Ref::none(), "Module.Create().ga", 18).is_empty());
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
    fn dotted_value_calls_are_not_exported_as_module_functions() {
        let members = exported_members(
            "return {\n Color = Color3.fromRGB(170, 0, 0),\n Size = Vector3.new(1, 2, 3),\n}",
        );
        assert!(members.contains_key("Color"));
        assert!(members.contains_key("Size"));
        assert!(!members.contains_key("fromRGB"));
        assert!(!members.contains_key("new"));
    }

    #[test]
    fn exports_frozen_setmetatable_registry_with_typed_interface() {
        // `return SomeTable` (not `return {`), wrapped in
        // table.freeze(setmetatable(...)) and typed by an exported interface.
        let source = "export type Registry = {\n\
             \tInitializeNPC: (npc: Model) -> StateProxy?,\n\
             \tCleanupNPC: (npc: Model) -> boolean,\n\
             \tGetCharacterAndPlayer: (target: Instance) -> (Model?, Player?),\n\
             \tSetTimed: (target: StateTarget, key: StateKey, duration: number) -> boolean,\n\
             \t[StateTarget]: StateProxy?,\n\
             }\n\
             const StatsRegistry: Registry = table.freeze(setmetatable({\n\
             \tInitializeNPC = initializeNPC,\n\
             \tCleanupNPC = cleanupNPC,\n\
             \tGetCharacterAndPlayer = function(target: Instance): (Model?, Player?)\n\
             \t\treturn getCharacterAndPlayer(target)\n\
             \tend,\n\
             \tSetTimed = setTimedState,\n\
             }, RegistryMetatable)) :: any\n\
             \n\
             return StatsRegistry\n";
        let members = exported_members(source);
        for member in ["InitializeNPC", "CleanupNPC", "GetCharacterAndPlayer", "SetTimed"] {
            assert!(members.contains_key(member), "missing {member} in {members:?}");
        }
        // Signatures come from the annotated interface, not just `field X`.
        assert!(members["SetTimed"].starts_with("function SetTimed("));
        // Metatable/wrapper identifiers are not module members.
        assert!(!members.contains_key("RegistryMetatable"));
        assert!(!members.contains_key("freeze"));
        assert!(!members.contains_key("setmetatable"));
    }

    #[test]
    fn metamethods_are_not_offered_as_module_members() {
        let source = "local Class = {}\n\
             Class.__index = Class\n\
             function Class.new()\nend\n\
             function Class:Destroy()\nend\n\
             return Class\n";
        let members = exported_members(source);
        assert!(members.contains_key("new"));
        assert!(members.contains_key("Destroy"));
        assert!(!members.contains_key("__index"));
    }

    #[test]
    fn dynamically_built_modules_suppress_unknown_member_warnings() {
        // Computed keys, forwarded requires, and callable modules have no
        // statically complete member list.
        assert!(has_dynamic_exports(
            "local M = {}\nfor _, name in ipairs(list) do\n\tM[name] = build(name)\nend\nreturn M",
        ));
        assert!(has_dynamic_exports("return require(script.Parent.Real)"));
        assert!(has_dynamic_exports("return function(a, b)\n\treturn a + b\nend"));
        // A plain static module, and the `Class.__index = Class` self
        // reference, must stay strictly checked.
        assert!(!has_dynamic_exports("local M = {}\nM.A = 1\nreturn M"));
        assert!(!has_dynamic_exports("local C = {}\nC.__index = C\nreturn C"));
    }

    #[test]
    fn no_unknown_member_warnings_for_dynamic_modules() {
        let module = Ref::new();
        let mut index = ProjectIndex::default();
        index.paths.insert(module, "Dynamic".into());
        index.module_refs.insert("dynamic".into(), module);
        let dynamic = "local M = {}\nM.Known = 1\nM[key] = 2\nreturn M";
        index.module_sources.insert(module, dynamic.into());
        index.modules.insert(
            "dynamic".into(),
            BTreeMap::from([("Known".into(), "field Known".into())]),
        );
        let warnings = index.diagnostics(
            Ref::none(),
            "const D = require(\"./Dynamic\")\nD.Whatever()",
        );
        assert!(
            !warnings.iter().any(|warning| warning.message.contains("Unknown member")),
            "dynamic module produced false positives: {warnings:?}",
        );
    }

    #[test]
    fn exports_members_assigned_after_a_returned_table_declaration() {
        let source = "local Module = {}\n\
             Module.Version = 3\n\
             function Module.Start(config)\nend\n\
             function Module:Stop()\nend\n\
             return Module\n";
        let members = exported_members(source);
        assert_eq!(members.get("Version"), Some(&"field Version".to_string()));
        assert_eq!(members.get("Start"), Some(&"function Start(config)".to_string()));
        assert!(members["Stop"].starts_with("method "));
    }

    #[test]
    fn dotted_function_with_typed_params_is_not_a_method() {
        // A ':' inside a parameter's type annotation must not be mistaken
        // for the owner/member separator — this is a dot function, not a
        // method, even though the signature contains a colon.
        let members = exported_members(
            "function Module.Ragdoll(Data: DataSettings)\nend\nreturn Module",
        );
        assert_eq!(
            members.get("Ragdoll"),
            Some(&"function Ragdoll(Data: DataSettings)".to_string())
        );
    }

    #[test]
    fn colon_method_with_typed_params_is_still_a_method() {
        let members = exported_members(
            "function Module:Ragdoll(Data: DataSettings)\nend\nreturn Module",
        );
        assert_eq!(
            members.get("Ragdoll"),
            Some(&"method function Ragdoll(Data: DataSettings)".to_string())
        );
    }

    #[test]
    fn nested_return_table_keys_are_not_module_exports() {
        let members = exported_members(
            "return {\n Name = 'Jun',\n Moves = {\n  Base = {\n   [1] = { KeyBind = 1 }\n  }\n }\n}",
        );
        assert!(members.contains_key("Name"));
        assert!(members.contains_key("Moves"));
        assert!(!members.contains_key("Base"));
        assert!(!members.contains_key("KeyBind"));
        let paths = returned_member_paths(
            "return {\n Moves = {\n  Base = {},\n  Ultimate = {},\n }\n}",
        );
        assert!(paths.contains_key("Moves.Base"));
        assert!(paths.contains_key("Moves.Ultimate"));
    }

    #[test]
    fn completes_nested_module_return_tables() {
        let module = Ref::new();
        let mut index = ProjectIndex::default();
        index.module_refs.insert("module".into(), module);
        index.module_member_paths.insert(module, BTreeMap::from([
            ("Moves.Base".into(), "table Base".into()),
            ("Moves.Ultimate".into(), "table Ultimate".into()),
        ]));
        // The module must be bound as a local before `Module.Moves.` is
        // allowed to resolve; a bare module name is not in scope.
        let source = "local Module = require(\"Module\")\nModule.Moves.";
        let suggestions = index.complete_at(Ref::none(), source, source.chars().count());
        assert!(suggestions.iter().any(|item| item.label == "Base"));
        assert!(suggestions.iter().any(|item| item.label == "Ultimate"));
    }

    #[test]
    fn completes_in_scope_locals_but_not_unbound_module_names() {
        let debris = Ref::new();
        let mut index = ProjectIndex::default();
        index.paths.insert(debris, "Debris".into());
        index.module_refs.insert("debris".into(), debris);
        index.module_sources.insert(debris, "return { CleanUp = true }".into());
        index.modules.insert("debris".into(), BTreeMap::from([("CleanUp".into(), "field CleanUp".into())]));

        // `Debris` exists as a ModuleScript, but nothing in this script bound
        // it, so it must not be offered as if it were a local variable.
        let unbound = index.complete_at(Ref::none(), "Deb", 3);
        assert!(
            !unbound.iter().any(|item| item.label == "Debris"),
            "unbound module offered as a local: {unbound:?}",
        );

        // After `local Debris = require(...)`, the name is a real local and
        // should complete with the module annotation.
        let bound = "local Debris = require(\"./Debris\")\nDeb";
        let suggestions = index.complete_at(Ref::none(), bound, bound.chars().count());
        let debris = suggestions.iter().find(|item| item.label == "Debris");
        assert!(debris.is_some(), "bound module missing from locals: {suggestions:?}");
        assert!(debris.unwrap().detail.contains("ModuleScript"));
    }

    #[test]
    fn module_members_require_an_in_scope_alias() {
        let inventory = Ref::new();
        let mut index = ProjectIndex::default();
        index.paths.insert(inventory, "ReplicatedStorage/Inventory".into());
        index.module_refs.insert("inventory".into(), inventory);
        index.module_sources.insert(inventory, "return { AddItem = function() end }".into());
        index.modules.insert(
            "inventory".into(),
            BTreeMap::from([("AddItem".into(), "function AddItem()".into())]),
        );

        // A module name without a local binding is not a variable; `Debris.`
        // style access must not invent module members.
        let unbound = "Inventory.AddI";
        assert!(
            index.complete_at(Ref::none(), unbound, unbound.chars().count())
                .iter().all(|item| item.label != "AddItem"),
            "unbound module members were offered",
        );

        let source = "local Inventory = require(\"./Inventory\")\nInventory.AddI";
        let suggestions = index.complete_at(Ref::none(), source, source.chars().count());
        assert!(suggestions.iter().any(|item| item.label == "AddItem"));
    }

    #[test]
    fn local_declarations_are_scoped_and_typed() {
        let index = ProjectIndex::default();
        let source = "if true then\nlocal inner = 1\nend\ninn";
        // `inner` was declared inside the block and is not visible after `end`.
        let outside = local_bindings_at(&index, source, source.chars().count(), Ref::none());
        assert!(
            !outside.iter().any(|binding| binding.name == "inner"),
            "block local leaked: {outside:?}",
        );

        let inside = "if true then\nlocal inner = 1\ninn";
        let in_scope = local_bindings_at(&index, inside, inside.chars().count(), Ref::none());
        assert!(in_scope.iter().any(|binding| binding.name == "inner"));

        let typed = "local part: BasePart\npar";
        let typed_locals = local_bindings_at(&index, typed, typed.chars().count(), Ref::none());
        let part = typed_locals.iter().find(|binding| binding.name == "part");
        assert_eq!(part.and_then(|binding| binding.instance_class.as_deref()), Some("BasePart"));

        let newed = "local part = Instance.new(\"Part\")\npar";
        let newed_locals = local_bindings_at(&index, newed, newed.chars().count(), Ref::none());
        assert_eq!(
            newed_locals.iter().find(|binding| binding.name == "part")
                .and_then(|binding| binding.instance_class.as_deref()),
            Some("Part"),
        );
    }

    #[test]
    fn function_params_loop_vars_and_self_are_completed_in_scope() {
        let index = ProjectIndex::default();
        let function_source = "local function go(foo: number, bar)\nfo";
        let locals = local_bindings_at(&index, function_source, function_source.chars().count(), Ref::none());
        assert!(locals.iter().any(|binding| binding.name == "foo"));
        assert!(locals.iter().any(|binding| binding.name == "bar"));
        assert!(locals.iter().any(|binding| binding.name == "go"));

        let after = "local function go(foo)\nend\nfo";
        assert!(
            !local_bindings_at(&index, after, after.chars().count(), Ref::none())
                .iter().any(|binding| binding.name == "foo"),
            "parameter leaked after function end",
        );

        let loop_source = "for k, v in pairs(items) do\nv";
        assert!(local_bindings_at(&index, loop_source, loop_source.chars().count(), Ref::none())
            .iter().any(|binding| binding.name == "v"));

        let method_source = "function Obj:Destroy(foo)\nsel";
        let method_locals = local_bindings_at(&index, method_source, method_source.chars().count(), Ref::none());
        assert!(method_locals.iter().any(|binding| binding.name == "self"));
        assert!(method_locals.iter().any(|binding| binding.name == "foo"));
    }

    #[test]
    fn global_functions_are_not_offered_as_locals() {
        let index = ProjectIndex::default();
        let global = "function Fred(a)\nFr";
        assert!(
            !local_bindings_at(&index, global, global.chars().count(), Ref::none())
                .iter().any(|binding| binding.name == "Fred"),
            "bare global function offered as a local",
        );
        let local = "local function Fred(a)\nFr";
        assert!(local_bindings_at(&index, local, local.chars().count(), Ref::none())
            .iter().any(|binding| binding.name == "Fred"));
    }

    #[test]
    fn instance_properties_come_from_scope_aware_locals() {
        let index = ProjectIndex::default();
        let source = "local part = Instance.new(\"Part\")\npart.Trans";
        let suggestions = index.complete_at(Ref::none(), source, source.chars().count());
        assert!(suggestions.iter().any(|item| item.label == "Transparency"));

        let typed = "local part: BasePart\npart.Anch";
        let typed_suggestions = index.complete_at(Ref::none(), typed, typed.chars().count());
        assert!(typed_suggestions.iter().any(|item| item.label == "Anchored"));

        // A plain ambiguous local (no known class) must not fall through to a
        // ModuleScript of the same name.
        let debris = Ref::new();
        let mut with_module = index;
        with_module.paths.insert(debris, "Debris".into());
        with_module.module_refs.insert("debris".into(), debris);
        with_module.module_sources.insert(debris, "return { CleanUp = true }".into());
        with_module.modules.insert("debris".into(), BTreeMap::from([("CleanUp".into(), "field CleanUp".into())]));
        let ambiguous = "local Debris = {}\nDebris.Cle";
        let ambiguous_suggestions = with_module.complete_at(Ref::none(), ambiguous, ambiguous.chars().count());
        assert!(
            !ambiguous_suggestions.iter().any(|item| item.label == "CleanUp"),
            "plain local fell back to module members: {ambiguous_suggestions:?}",
        );
    }

    #[test]
    fn infers_objects_returned_by_module_constructors() {
        let module = Ref::new();
        let consumer = Ref::new();
        let module_source = "function Weld.new(part) end\nfunction Weld:Destroy() end\nreturn Weld";
        let mut index = ProjectIndex::default();
        index.module_refs.insert("weldmodule".into(), module);
        index.module_sources.insert(module, module_source.into());
        index.paths.insert(consumer, "Controller".into());
        let source = "local Module = require(\"WeldModule\")\nlocal weld = Module.new(part)\nweld:D";
        let suggestions = index.complete_at(consumer, source, source.chars().count());
        assert!(suggestions.iter().any(|item| item.label == "Destroy"));
        let dot_source = "local Module = require(\"WeldModule\")\nlocal weld = Module.new(part)\nweld.D";
        assert!(!index.complete_at(consumer, dot_source, dot_source.chars().count())
            .iter().any(|item| item.label == "Destroy"));
    }

    #[test]
    fn infers_oop_receiver_and_preset_fields() {
        let source = "local Defaults = {\n Speed = 10,\n Enabled = true,\n}\n\
            local Weld = {}\nWeld.__index = Weld\n\
            function Weld.new()\n local object = setmetatable(table.clone(Defaults), Weld)\n object.Part0 = nil\n return object\nend\n\
            function Weld:Destroy() end";
        let members = constructed_object_members(source);
        assert!(members.contains_key("Speed"));
        assert!(members.contains_key("Enabled"));
        assert!(members.contains_key("Part0"));
        assert!(members.contains_key("Destroy"));
        assert!(!members.contains_key("new"));
    }

    #[test]
    fn color3_completion_preserves_roblox_casing() {
        let index = ProjectIndex::default();
        let suggestions = index.complete_at(Ref::none(), "Color3.fr", 9);
        assert!(suggestions.iter().any(|item| item.label == "fromRGB"));
        assert!(!suggestions.iter().any(|item| item.label == "fromrgb"));
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

    #[test]
    fn generated_api_covers_libraries_datatypes_and_globals() {
        let index = ProjectIndex::default();

        // `task.wait` and stock library members.
        let task = index.complete_at(Ref::none(), "task.wa", "task.wa".chars().count());
        assert!(task.iter().any(|item| item.label == "wait"), "{task:?}");

        // `Vector3` statics (dot) vs methods (colon only).
        let vector_new = index.complete_at(Ref::none(), "Vector3.ne", "Vector3.ne".chars().count());
        assert!(vector_new.iter().any(|item| item.label == "new"), "{vector_new:?}");
        let vector_dot = index.complete_at(Ref::none(), "Vector3.Cr", "Vector3.Cr".chars().count());
        assert!(
            !vector_dot.iter().any(|item| item.label == "Cross"),
            "datatype methods must not complete on dot: {vector_dot:?}"
        );
        let vector_colon = index.complete_at(Ref::none(), "Vector3:Cr", "Vector3:Cr".chars().count());
        assert!(vector_colon.iter().any(|item| item.label == "Cross"), "{vector_colon:?}");

        // `script.Parent` (property/dot) and `script:Destroy` (method/colon).
        let script_parent = index.complete_at(Ref::none(), "script.Par", "script.Par".chars().count());
        assert!(script_parent.iter().any(|item| item.label == "Parent"), "{script_parent:?}");
        let script_dot = index.complete_at(Ref::none(), "script.Des", "script.Des".chars().count());
        assert!(
            !script_dot.iter().any(|item| item.label == "Destroy"),
            "methods must not complete on dot: {script_dot:?}"
        );
        let script_colon = index.complete_at(Ref::none(), "script:Des", "script:Des".chars().count());
        assert!(script_colon.iter().any(|item| item.label == "Destroy"), "{script_colon:?}");
    }

    #[test]
    fn typed_annotations_and_service_chains_infer_types() {
        let index = ProjectIndex::default();

        // `local Player : Player = Actor.Player` — annotation types the local,
        // so it is offered and gets member completions.
        let declared = "local Player : Player = Actor.Player\nPlay";
        let suggestions = index.complete_at(Ref::none(), declared, declared.chars().count());
        assert!(suggestions.iter().any(|item| item.label == "Player"), "{suggestions:?}");

        // function parameters with typed annotations are in scope and typed.
        let params = "function test(Player: Player)\nPlay";
        let param_locals = local_bindings_at(&index, params, params.chars().count(), Ref::none());
        let player = param_locals.iter().find(|binding| binding.name == "Player");
        assert_eq!(
            player.and_then(|binding| binding.instance_class.as_deref()),
            Some("Player")
        );

        // `game:GetService("Players").LocalPlayer` → Player, methods via colon.
        let chain = "local p = game:GetService(\"Players\").LocalPlayer\np:Dis";
        let chain_suggestions = index.complete_at(Ref::none(), chain, chain.chars().count());
        assert!(
            chain_suggestions.iter().any(|item| item.label == "DistanceFromCharacter"),
            "{chain_suggestions:?}"
        );

        // `game:GetService("RunService")` → RunService members (event dot,
        // method colon).
        let run = "local RunService = game:GetService(\"RunService\")\nRunService.Hear";
        let run_suggestions = index.complete_at(Ref::none(), run, run.chars().count());
        assert!(run_suggestions.iter().any(|item| item.label == "Heartbeat"), "{run_suggestions:?}");
        let run_method = "local RunService = game:GetService(\"RunService\")\nRunService:IsS";
        let method_suggestions = index.complete_at(Ref::none(), run_method, run_method.chars().count());
        assert!(method_suggestions.iter().any(|item| item.label == "IsServer"), "{method_suggestions:?}");
    }

    #[test]
    fn nested_type_flow_and_enum_members_complete() {
        let index = ProjectIndex::default();

        // Enum. → enum names; Enum.KeyCode. → items.
        let enum_names = index.complete_at(Ref::none(), "Enum.KeyC", "Enum.KeyC".chars().count());
        assert!(enum_names.iter().any(|item| item.label == "KeyCode"), "{enum_names:?}");
        let enum_items = index.complete_at(Ref::none(), "Enum.KeyCode.A", "Enum.KeyCode.A".chars().count());
        assert!(enum_items.iter().any(|item| item.label == "A"), "{enum_items:?}");

        // player.CharacterAdded:Co → RBXScriptSignal.Connect (colon method),
        // while the dot form stays member-free.
        let signal = "local p = game:GetService(\"Players\").LocalPlayer\np.CharacterAdded:Co";
        let signal_suggestions = index.complete_at(Ref::none(), signal, signal.chars().count());
        assert!(signal_suggestions.iter().any(|item| item.label == "Connect"), "{signal_suggestions:?}");
        let signal_dot = "local p = game:GetService(\"Players\").LocalPlayer\np.CharacterAdded.Co";
        let dot_suggestions = index.complete_at(Ref::none(), signal_dot, signal_dot.chars().count());
        assert!(
            !dot_suggestions.iter().any(|item| item.label == "Connect"),
            "signal methods must not complete on dot: {dot_suggestions:?}"
        );
    }

    /// Builds a small place: services with children plus a Script to edit.
    fn test_place_dom() -> WeakDom {
        WeakDom::new(
            InstanceBuilder::new("DataModel")
                .with_name("game")
                .with_child(
                    InstanceBuilder::new("ReplicatedStorage")
                        .with_name("ReplicatedStorage")
                        .with_child(
                            InstanceBuilder::new("RemoteEvent").with_name("ClientRenderRequest"),
                        )
                        .with_child(InstanceBuilder::new("Folder").with_name("Things")),
                )
                .with_child(
                    InstanceBuilder::new("Workspace")
                        .with_name("Workspace")
                        .with_child(InstanceBuilder::new("Terrain").with_name("Terrain"))
                        .with_child(InstanceBuilder::new("Part").with_name("Baseplate")),
                )
                .with_child(InstanceBuilder::new("ServerStorage").with_name("ServerStorage"))
                .with_child(
                    InstanceBuilder::new("ServerScriptService")
                        .with_name("ServerScriptService")
                        .with_child(InstanceBuilder::new("Script").with_name("Main")),
                ),
        )
    }

    fn test_child_ref(dom: &WeakDom, parent: Ref, name: &str) -> Ref {
        dom.get_by_ref(parent)
            .expect("test parent")
            .children()
            .iter()
            .find(|child| dom.get_by_ref(**child).expect("test child").name == name)
            .map(|child| **child)
            .expect("test child missing")
    }

    #[test]
    fn service_and_instance_paths_complete_and_infer() {
        let dom = test_place_dom();
        let index = ProjectIndex::build(&dom);
        let service = test_child_ref(&dom, dom.root_ref(), "ServerScriptService");
        let script = test_child_ref(&dom, service, "Main");

        // `game.ReplicatedStorage` / `game.ServerStorage` complete from the
        // open place even though the API dump has no such members.
        let services = index.complete_at(script, "game.Repli", "game.Repli".chars().count());
        assert!(services.iter().any(|item| item.label == "ReplicatedStorage"), "{services:?}");
        let storage = index.complete_at(script, "game.ServerSt", "game.ServerSt".chars().count());
        assert!(storage.iter().any(|item| item.label == "ServerStorage"), "{storage:?}");

        // Children of services complete: `ReplicatedStorage.Things`.
        let via_service = "local rs = game:GetService(\"ReplicatedStorage\")\nrs.Thi";
        let things = index.complete_at(script, via_service, via_service.chars().count());
        assert!(things.iter().any(|item| item.label == "Things"), "{things:?}");

        // A full DataModel path needs no annotation:
        // `game.ReplicatedStorage.ClientRenderRequest` -> RemoteEvent.
        let remote = "local request = game.ReplicatedStorage.ClientRenderRequest\nrequest:Fi";
        let fire = index.complete_at(script, remote, remote.chars().count());
        assert!(fire.iter().any(|item| item.label == "FireServer"), "{fire:?}");

        // Nested paths flow too.
        let nested = "game.ReplicatedStorage.ClientRenderRequest:Fi";
        let nested_fire = index.complete_at(script, nested, nested.chars().count());
        assert!(nested_fire.iter().any(|item| item.label == "FireServer"), "{nested_fire:?}");

        // `:WaitForChild("Things")` with a literal types the real child.
        let waited = "local folder = game.ReplicatedStorage:WaitForChild(\"Things\")\nfolder:Find";
        let found = index.complete_at(script, waited, waited.chars().count());
        assert!(found.iter().any(|item| item.label == "FindFirstChild"), "{found:?}");

        // `:FindFirstChildOfClass("RemoteEvent")` types by the class argument.
        let of_class = "local remote = game.ReplicatedStorage:FindFirstChildOfClass(\"RemoteEvent\")\nremote:Fi";
        let fired = index.complete_at(script, of_class, of_class.chars().count());
        assert!(fired.iter().any(|item| item.label == "FireServer"), "{fired:?}");

        // `script.Parent` climbs to the real parent service.
        let parented = "local owner = script.Parent\nowner:Wait";
        let wait = index.complete_at(script, parented, parented.chars().count());
        assert!(wait.iter().any(|item| item.label == "WaitForChild"), "{wait:?}");
    }
}
