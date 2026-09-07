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
    fn removes_up_to_four_spaces() {
        let mut text = "    one\n  two".to_string();
        let end = text.chars().count();
        indent_lines(&mut text, 0, end, true);
        assert_eq!(text, "one\ntwo");
    }
}
