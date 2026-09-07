//! Selection-aware text transformations used by the Luau editor.

/// Indent or unindent every line touched by a character-indexed selection.
/// Returns updated anchor/primary positions while preserving selection direction.
pub fn indent_lines(
    source: &mut String,
    anchor: usize,
    primary: usize,
    unindent: bool,
) -> (usize, usize) {
    let total = source.chars().count();
    let low = anchor.min(primary).min(total);
    let high = anchor.max(primary).min(total);
    let chars: Vec<char> = source.chars().collect();

    let first_start = chars[..low].iter().rposition(|c| *c == '\n').map_or(0, |p| p + 1);
    let mut last_limit = high;
    // A selection ending at column zero does not include that final line.
    if high > low && high > 0 && chars.get(high - 1) == Some(&'\n') {
        last_limit = high - 1;
    }

    let mut starts = vec![first_start];
    for (index, ch) in chars.iter().enumerate().take(last_limit) {
        if *ch == '\n' && index + 1 < last_limit {
            starts.push(index + 1);
        }
    }

    #[derive(Clone, Copy)]
    struct Edit { start: usize, remove: usize, insert: usize }
    let edits: Vec<Edit> = starts.into_iter().map(|start| {
        if !unindent {
            Edit { start, remove: 0, insert: 1 }
        } else if chars.get(start) == Some(&'\t') {
            Edit { start, remove: 1, insert: 0 }
        } else {
            let spaces = chars.iter().skip(start).take(4).take_while(|c| **c == ' ').count();
            Edit { start, remove: spaces, insert: 0 }
        }
    }).filter(|edit| edit.remove != 0 || edit.insert != 0).collect();

    let map_position = |mut position: usize| {
        for edit in &edits {
            if position < edit.start {
                continue;
            }
            if position <= edit.start + edit.remove {
                position = edit.start + edit.insert;
            } else if edit.insert >= edit.remove {
                position += edit.insert - edit.remove;
            } else {
                position -= edit.remove - edit.insert;
            }
        }
        position
    };
    let new_anchor = map_position(anchor);
    let new_primary = map_position(primary);

    for edit in edits.iter().rev() {
        let start_byte = char_to_byte(source, edit.start);
        let end_byte = char_to_byte(source, edit.start + edit.remove);
        let replacement = if edit.insert == 1 { "\t" } else { "" };
        source.replace_range(start_byte..end_byte, replacement);
    }
    (new_anchor, new_primary)
}

/// Apply code-editor conveniences after TextEdit/Android IME inserted text.
/// Returns the desired caret position when it added or skipped a delimiter.
pub fn enhance_typed_edit(before: &str, after: &mut String, cursor: usize) -> Option<usize> {
    let before_chars: Vec<char> = before.chars().collect();
    let after_chars: Vec<char> = after.chars().collect();
    // For a one-character keypress, the caret disambiguates repeated closing
    // characters that an ordinary longest-prefix diff cannot distinguish.
    let single_insert = if after_chars.len() == before_chars.len() + 1 && cursor > 0 {
        let mut without = after_chars.clone();
        let inserted = without.remove(cursor - 1);
        (without == before_chars).then_some((cursor - 1, inserted))
    } else {
        None
    };
    let (prefix, inserted, removed) = if let Some((at, ch)) = single_insert {
        (at, vec![ch], 0)
    } else {
        let mut prefix = 0;
        while prefix < before_chars.len()
            && prefix < after_chars.len()
            && before_chars[prefix] == after_chars[prefix]
        {
            prefix += 1;
        }
        let mut suffix = 0;
        while suffix < before_chars.len().saturating_sub(prefix)
            && suffix < after_chars.len().saturating_sub(prefix)
            && before_chars[before_chars.len() - 1 - suffix]
                == after_chars[after_chars.len() - 1 - suffix]
        {
            suffix += 1;
        }
        (
            prefix,
            after_chars[prefix..after_chars.len().saturating_sub(suffix)].to_vec(),
            before_chars.len().saturating_sub(prefix + suffix),
        )
    };

    // Pair a single opening delimiter without interfering with paste or
    // replacement edits. Quotes after a backslash are intentionally ignored.
    if removed == 0 && inserted.len() == 1 && cursor == prefix + 1 {
        let opener = inserted[0];
        let closer = match opener {
            '(' => ')', '[' => ']', '{' => '}', '"' => '"', '\'' => '\'', '`' => '`',
            _ => '\0',
        };
        // Typing an existing closing delimiter moves over it instead of
        // leaving two copies. This check comes first because quotes are both
        // opening and closing delimiters.
        if matches!(opener, ')' | ']' | '}' | '"' | '\'' | '`')
            && before_chars.get(prefix) == Some(&opener)
        {
            let start = char_to_byte(after, prefix);
            let end = char_to_byte(after, prefix + 1);
            after.replace_range(start..end, "");
            return Some(cursor);
        }

        if closer != '\0' && !(matches!(opener, '"' | '\'' | '`') && prefix > 0 && after_chars[prefix - 1] == '\\') {
            let byte = char_to_byte(after, cursor);
            after.insert(byte, closer);
            return Some(cursor);
        }
    }

    // Continue the previous line's indentation after Enter. Block-opening
    // lines receive one extra tab, matching common Luau formatting.
    if removed == 0 && inserted == ['\n'] && cursor == prefix + 1 {
        let line_start = before_chars[..prefix].iter().rposition(|c| *c == '\n').map_or(0, |p| p + 1);
        let line: String = before_chars[line_start..prefix].iter().collect();
        let indent: String = line.chars().take_while(|c| matches!(c, ' ' | '\t')).collect();
        let trimmed = line.trim_end();
        let opens_block = trimmed.ends_with("then") || trimmed.ends_with("do")
            || trimmed.ends_with('{') || trimmed.ends_with('[') || trimmed.ends_with('(')
            || trimmed.starts_with("function ") || trimmed.starts_with("local function ")
            || trimmed.starts_with("const function ");
        let addition = if opens_block { format!("{indent}\t") } else { indent };
        if !addition.is_empty() {
            let byte = char_to_byte(after, cursor);
            after.insert_str(byte, &addition);
            return Some(cursor + addition.chars().count());
        }
    }
    None
}

fn char_to_byte(source: &str, index: usize) -> usize {
    source.char_indices().nth(index).map_or(source.len(), |(byte, _)| byte)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indents_and_unindents_selected_lines() {
        let mut text = "one\ntwo\nthree".to_string();
        let range = indent_lines(&mut text, 1, 7, false);
        assert_eq!(text, "\tone\n\ttwo\nthree");
        let range = indent_lines(&mut text, range.0, range.1, true);
        assert_eq!(text, "one\ntwo\nthree");
        assert_eq!(range, (1, 7));
    }

    #[test]
    fn pairs_delimiters_and_keeps_caret_inside() {
        let mut after = "call(".to_string();
        assert_eq!(enhance_typed_edit("call", &mut after, 5), Some(5));
        assert_eq!(after, "call()");
    }

    #[test]
    fn indents_after_luau_block_openers() {
        let mut after = "\tif ready then\n".to_string();
        let cursor = after.chars().count();
        assert_eq!(
            enhance_typed_edit("\tif ready then", &mut after, cursor),
            Some(cursor + 2)
        );
        assert_eq!(after, "\tif ready then\n\t\t");
    }

    #[test]
    fn skips_an_existing_closer() {
        let mut after = "call())".to_string();
        assert_eq!(enhance_typed_edit("call()", &mut after, 6), Some(6));
        assert_eq!(after, "call()");
    }

    #[test]
    fn removes_up_to_four_spaces() {
        let mut text = "    one\n  two".to_string();
        let end = text.chars().count();
        indent_lines(&mut text, 0, end, true);
        assert_eq!(text, "one\ntwo");
    }
}
