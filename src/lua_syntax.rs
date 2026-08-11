use bevy_egui::egui::text::LayoutJob;
use bevy_egui::egui::{Color32, FontId, TextFormat};
use std::collections::HashSet;

pub fn highlight_lua(text: &str, font_size: f32, search_term: Option<&str>) -> LayoutJob {
    let font = FontId::monospace(font_size);
    let mut job = LayoutJob::default();

    let fmt_default = TextFormat::simple(font.clone(), Color32::from_rgb(220, 220, 220));
    let fmt_keyword = TextFormat::simple(font.clone(), Color32::from_rgb(240, 100, 110)); // Red/Coral
    let fmt_type = TextFormat::simple(font.clone(), Color32::from_rgb(100, 180, 246));    // Sky Blue
    let fmt_builtin = TextFormat::simple(font.clone(), Color32::from_rgb(186, 104, 200)); // Purple
    let fmt_string = TextFormat::simple(font.clone(), Color32::from_rgb(129, 199, 132));  // Green
    let fmt_comment = TextFormat::simple(font.clone(), Color32::from_rgb(130, 145, 155)); // Gray
    let fmt_number = TextFormat::simple(font.clone(), Color32::from_rgb(255, 183, 77));   // Orange
    let fmt_bool = TextFormat::simple(font.clone(), Color32::from_rgb(255, 213, 79));     // Amber

    let fmt_search = TextFormat {
        font_id: font.clone(),
        color: Color32::BLACK,
        background: Color32::from_rgb(255, 235, 59),
        ..Default::default()
    };

    let keywords: HashSet<&str> = [
        "and", "break", "continue", "do", "else", "elseif", "end", "export",
        "for", "function", "goto", "if", "in", "local", "not", "or",
        "repeat", "return", "then", "type", "until", "while",
    ].into_iter().collect();

    let bools: HashSet<&str> = ["true", "false", "nil"].into_iter().collect();

    let types: HashSet<&str> = [
        "string", "number", "boolean", "table", "any", "nil", "thread",
        "userdata", "buffer", "vector", "Instance", "Vector3", "Vector2",
        "CFrame", "Color3", "UDim", "UDim2", "Ray", "Enum", "BrickColor",
        "TweenInfo", "PhysicalProperties", "NumberRange", "NumberSequence",
        "ColorSequence", "Rect", "Faces", "Axes",
    ].into_iter().collect();

    let builtins: HashSet<&str> = [
        "game", "workspace", "script", "self", "math", "table", "string",
        "task", "coroutine", "os", "debug", "utf8", "bit32", "buffer",
        "print", "warn", "error", "pairs", "ipairs", "next", "typeof",
        "type", "pcall", "xpcall", "select", "tonumber", "tostring",
        "rawget", "rawset", "rawequal", "setmetatable", "getmetatable",
        "require", "assert",
    ].into_iter().collect();

    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    let search_lower = search_term.map(|s| s.to_lowercase());

    while i < len {
        // Check for search term match
        if let Some(ref st) = search_lower {
            if !st.is_empty() {
                let remaining: String = chars[i..].iter().take(st.chars().count()).collect();
                if remaining.to_lowercase() == *st {
                    job.append(&remaining, 0.0, fmt_search.clone());
                    i += st.chars().count();
                    continue;
                }
            }
        }

        // Comments: --[[ ... ]] or -- ...
        if i + 1 < len && chars[i] == '-' && chars[i + 1] == '-' {
            let start = i;
            i += 2;
            if i + 1 < len && chars[i] == '[' && chars[i + 1] == '[' {
                i += 2;
                while i + 1 < len && !(chars[i] == ']' && chars[i + 1] == ']') {
                    i += 1;
                }
                if i + 1 < len {
                    i += 2;
                }
            } else {
                while i < len && chars[i] != '\n' {
                    i += 1;
                }
            }
            let comment: String = chars[start..i].iter().collect();
            job.append(&comment, 0.0, fmt_comment.clone());
            continue;
        }

        // Multiline strings: [[ ... ]]
        if i + 1 < len && chars[i] == '[' && chars[i + 1] == '[' {
            let start = i;
            i += 2;
            while i + 1 < len && !(chars[i] == ']' && chars[i + 1] == ']') {
                i += 1;
            }
            if i + 1 < len {
                i += 2;
            }
            let s: String = chars[start..i].iter().collect();
            job.append(&s, 0.0, fmt_string.clone());
            continue;
        }

        // Quoted strings: "..." or '...'
        if chars[i] == '"' || chars[i] == '\'' {
            let quote = chars[i];
            let start = i;
            i += 1;
            let mut escaped = false;
            while i < len {
                let c = chars[i];
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == quote || c == '\n' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            let s: String = chars[start..i].iter().collect();
            job.append(&s, 0.0, fmt_string.clone());
            continue;
        }

        // Numbers: 0x... or 123.45
        if chars[i].is_ascii_digit() || (chars[i] == '.' && i + 1 < len && chars[i + 1].is_ascii_digit()) {
            let start = i;
            if chars[i] == '0' && i + 1 < len && (chars[i + 1] == 'x' || chars[i + 1] == 'X') {
                i += 2;
                while i < len && chars[i].is_ascii_hexdigit() {
                    i += 1;
                }
            } else {
                while i < len && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == 'e' || chars[i] == 'E' || chars[i] == '_') {
                    i += 1;
                }
            }
            let num: String = chars[start..i].iter().collect();
            job.append(&num, 0.0, fmt_number.clone());
            continue;
        }

        // Identifiers and Keywords
        if chars[i].is_alphabetic() || chars[i] == '_' {
            let start = i;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let format = if keywords.contains(word.as_str()) {
                fmt_keyword.clone()
            } else if bools.contains(word.as_str()) {
                fmt_bool.clone()
            } else if types.contains(word.as_str()) {
                fmt_type.clone()
            } else if builtins.contains(word.as_str()) {
                fmt_builtin.clone()
            } else {
                fmt_default.clone()
            };
            job.append(&word, 0.0, format);
            continue;
        }

        // Punctuation and whitespace
        let c = chars[i];
        job.append(&c.to_string(), 0.0, fmt_default.clone());
        i += 1;
    }

    job
}
