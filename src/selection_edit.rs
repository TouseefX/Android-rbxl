//! Selection-aware text transformations used by the Luau editor.

use crate::app::{INDENT_UNIT, INDENT_WIDTH};

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

    // Line starts touched by the selection. `first_start` can coincide with a
    // newline-derived start (a selection beginning exactly at column zero), so
    // duplicates must be dropped: indenting the same line twice corrupted the
    // text and left the other selected lines untouched.
    let mut starts = vec![first_start];
    for (index, ch) in chars.iter().enumerate().take(last_limit) {
        if *ch == '\n' && index + 1 < last_limit && index + 1 != first_start {
            starts.push(index + 1);
        }
    }
    starts.dedup();

    #[derive(Clone, Copy)]
    struct Edit { start: usize, remove: usize, insert: usize }
    let edits: Vec<Edit> = starts.into_iter().map(|start| {
        if !unindent {
            Edit { start, remove: 0, insert: INDENT_WIDTH }
        } else if chars.get(start) == Some(&'\t') {
            Edit { start, remove: 1, insert: 0 }
        } else {
            let spaces = chars.iter().skip(start).take(INDENT_WIDTH)
                .take_while(|c| **c == ' ').count();
            Edit { start, remove: spaces, insert: 0 }
        }
    }).filter(|edit| edit.remove != 0 || edit.insert != 0).collect();

    // Every `edit.start` is in ORIGINAL coordinates, so the comparison must
    // use the original position and only the accumulated delta is applied at
    // the end. Mutating `position` inside the loop made it drift past later
    // edit starts and shift twice.
    let map_position = |position: usize| {
        let mut delta: isize = 0;
        for edit in &edits {
            if position < edit.start {
                continue;
            }
            if position <= edit.start + edit.remove {
                delta = (edit.start + edit.insert) as isize - position as isize;
                break;
            }
            delta += edit.insert as isize - edit.remove as isize;
        }
        (position as isize + delta).max(0) as usize
    };
    let new_anchor = map_position(anchor);
    let new_primary = map_position(primary);

    for edit in edits.iter().rev() {
        let start_byte = char_to_byte(source, edit.start);
        let end_byte = char_to_byte(source, edit.start + edit.remove);
        let replacement = if edit.insert > 0 { INDENT_UNIT } else { "" };
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
        let addition = if opens_block { format!("{indent}{INDENT_UNIT}") } else { indent };
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
        assert_eq!(text, "    one\n    two\nthree");
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
        let mut after = "    if ready then\n".to_string();
        let cursor = after.chars().count();
        assert_eq!(
            enhance_typed_edit("    if ready then", &mut after, cursor),
            Some(cursor + 8)
        );
        assert_eq!(after, "    if ready then\n        ");
    }

    #[test]
    fn skips_an_existing_closer() {
        let mut after = "call())".to_string();
        assert_eq!(enhance_typed_edit("call()", &mut after, 6), Some(6));
        assert_eq!(after, "call()");
    }

    #[test]
    fn indents_every_selected_line_exactly_once() {
        // A selection starting at column zero makes the first line start
        // coincide with a newline-derived one. Before dedup that indented the
        // second line twice and skipped the first.
        let mut text = "one\ntwo\nthree".to_string();
        let range = indent_lines(&mut text, 0, 8, false);
        assert_eq!(text, "    one\n    two\nthree");
        indent_lines(&mut text, range.0, range.1, true);
        assert_eq!(text, "one\ntwo\nthree");
    }

    #[test]
    fn maps_positions_across_multiple_indents() {
        // Positions past several edits must shift by the summed delta, not be
        // re-shifted after drifting past a later edit's start offset.
        let mut text = "one\ntwo\nthree".to_string();
        let range = indent_lines(&mut text, 1, 7, false);
        assert_eq!(text, "    one\n    two\nthree");
        // 'n' of "one" moves 1 -> 5; the newline after "two" moves 7 -> 15.
        assert_eq!(range, (5, 15));
    }

    #[test]
    fn removes_up_to_four_spaces() {
        let mut text = "    one\n  two".to_string();
        let end = text.chars().count();
        indent_lines(&mut text, 0, end, true);
        assert_eq!(text, "one\ntwo");
    }
}
