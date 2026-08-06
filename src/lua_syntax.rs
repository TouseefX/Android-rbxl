use egui_code_editor::Syntax;
use std::collections::BTreeSet;

/// egui_code_editor ships no Lua preset; its Syntax type is just a
/// keyword-set builder, so a Luau one is a few lines.
pub fn luau_syntax() -> Syntax {
    Syntax {
        language: "Luau",
        case_sensitive: true,
        comment: "--",
        comment_multiline: ["--[[", "]]"],
        hyperlinks: BTreeSet::new(),
        quotes: BTreeSet::from(['"', '\'']),
        keywords: BTreeSet::from([
            "and", "break", "continue", "do", "else", "elseif", "end", "export",
            "false", "for", "function", "goto", "if", "in", "local", "nil",
            "not", "or", "repeat", "return", "then", "true", "type", "until", "while",
        ]),
        types: BTreeSet::from([
            "string", "number", "boolean", "table", "nil", "any", "Instance",
            "Vector3", "CFrame", "Color3", "UDim2", "Enum",
        ]),
        special: BTreeSet::from(["self", "script", "game", "workspace"]),
    }
}
