//! Markdown list and quote continuation on Enter.

/// What Enter should do on the current line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnEnter {
    /// Insert this text (a newline plus the next marker).
    Continue(String),
    /// The item is empty: clear its marker and end the list.
    EndList,
    /// Not a list line: a normal newline.
    Plain,
}

/// Decides Enter from the text before and after the cursor on its line.
pub fn on_enter(before: &str, after: &str) -> OnEnter {
    let body = before.trim_start_matches([' ', '\t']);
    let indent = &before[..before.len() - body.len()];
    let Some((marker_len, next)) = parse_marker(body) else {
        return OnEnter::Plain;
    };
    if body[marker_len..].trim().is_empty() && after.trim().is_empty() {
        return OnEnter::EndList;
    }
    OnEnter::Continue(format!("\n{indent}{next}"))
}

/// Length of the list or quote marker at the start of `body`, and the marker for the next line.
fn parse_marker(body: &str) -> Option<(usize, String)> {
    if body.starts_with("> ") {
        return Some((2, "> ".to_owned()));
    }
    let bytes = body.as_bytes();
    if let [bullet @ (b'-' | b'*' | b'+'), b' ', ..] = bytes {
        let bullet = *bullet as char;
        let task = ["[ ] ", "[x] ", "[X] "]
            .iter()
            .any(|t| body[2..].starts_with(t));
        return Some(if task {
            (6, format!("{bullet} [ ] "))
        } else {
            (2, format!("{bullet} "))
        });
    }
    let digits = body.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=9).contains(&digits)
        && let [delim @ (b'.' | b')'), b' ', ..] = &bytes[digits..]
    {
        let number: u64 = body[..digits].parse().ok()?;
        return Some((digits + 2, format!("{}{} ", number + 1, *delim as char)));
    }
    None
}

/// Ctrl+L: adds a checkbox to the line, or toggles an existing one.
pub fn toggle_task(line: &str) -> String {
    let body = line.trim_start_matches([' ', '\t']);
    let indent = &line[..line.len() - body.len()];
    let (marker, rest) = body.split_at(list_marker_len(body));
    let marker = if marker.is_empty() { "- " } else { marker };
    let toggled = if let Some(text) = rest.strip_prefix("[ ]") {
        format!("[x]{text}")
    } else if let Some(text) = rest
        .strip_prefix("[x]")
        .or_else(|| rest.strip_prefix("[X]"))
    {
        format!("[ ]{text}")
    } else {
        format!("[ ] {rest}")
    };
    format!("{indent}{marker}{toggled}")
}

/// Length of a bullet or number marker such as `- ` or `12. `, without any checkbox.
fn list_marker_len(body: &str) -> usize {
    let bytes = body.as_bytes();
    if let [b'-' | b'*' | b'+', b' ', ..] = bytes {
        return 2;
    }
    let digits = body.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=9).contains(&digits) && matches!(&bytes[digits..], [b'.' | b')', b' ', ..]) {
        return digits + 2;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cont(s: &str) -> OnEnter {
        OnEnter::Continue(s.to_owned())
    }

    #[test]
    fn continues_bullets_with_indent() {
        assert_eq!(on_enter("- item", ""), cont("\n- "));
        assert_eq!(on_enter("  * item", ""), cont("\n  * "));
        assert_eq!(on_enter("\t+ item", ""), cont("\n\t+ "));
    }

    #[test]
    fn increments_ordered_lists() {
        assert_eq!(on_enter("1. one", ""), cont("\n2. "));
        assert_eq!(on_enter("  9) nine", ""), cont("\n  10) "));
    }

    #[test]
    fn new_tasks_start_unchecked() {
        assert_eq!(on_enter("- [x] done", ""), cont("\n- [ ] "));
        assert_eq!(on_enter("- [ ] todo", ""), cont("\n- [ ] "));
    }

    #[test]
    fn continues_quotes() {
        assert_eq!(on_enter("> quote", ""), cont("\n> "));
    }

    #[test]
    fn empty_item_ends_list() {
        for line in ["- ", "  * ", "1. ", "- [ ] ", "> "] {
            assert_eq!(on_enter(line, ""), OnEnter::EndList, "{line:?}");
        }
    }

    #[test]
    fn splitting_an_item_continues() {
        assert_eq!(on_enter("- ", "text"), cont("\n- "));
        assert_eq!(on_enter("- ab", "cd"), cont("\n- "));
    }

    #[test]
    fn toggle_task_starts_and_cycles_checkboxes() {
        assert_eq!(toggle_task(""), "- [ ] ");
        assert_eq!(toggle_task("buy milk"), "- [ ] buy milk");
        assert_eq!(toggle_task("  - buy milk"), "  - [ ] buy milk");
        assert_eq!(toggle_task("* item"), "* [ ] item");
        assert_eq!(toggle_task("1. step"), "1. [ ] step");
        assert_eq!(toggle_task("- [ ] todo"), "- [x] todo");
        assert_eq!(toggle_task("  - [x] done"), "  - [ ] done");
        assert_eq!(toggle_task("- [X] done"), "- [ ] done");
    }

    #[test]
    fn plain_lines_are_plain() {
        for line in [
            "",
            "plain",
            "---",
            "**bold**",
            "-no space",
            "1.5 kg",
            "#tag",
        ] {
            assert_eq!(on_enter(line, ""), OnEnter::Plain, "{line:?}");
        }
    }
}
