use std::{borrow::Borrow, ops::Range};

use lsp_types::Position;

#[derive(Debug)]
pub struct OffsetMap<T>
where
    T: Borrow<str>,
{
    text: T,
    line_ranges: Vec<Range<usize>>,
}

impl<T: Borrow<str>> OffsetMap<T> {
    pub fn new(text: T) -> Self {
        let mut line_ranges = Vec::new();

        let mut line_start: Option<usize> = None;
        let mut last_char: Option<(usize, char)> = None;

        let mut char_iter = text.borrow().char_indices().peekable();

        while let Some((pos, c)) = char_iter.next() {
            if line_start.is_none() {
                line_start = Some(pos);
            }
            last_char = Some((pos, c));

            let mut is_newline = false;

            if c == '\n' {
                is_newline = true;
            } else if c == '\r' {
                if char_iter.peek().filter(|(_, pc)| *pc == '\n').is_some() {
                    continue;
                }
                is_newline = true;
            }

            if is_newline {
                let start = line_start.expect("line_start should be always initialized");
                assert!(
                    text.borrow().is_char_boundary(start),
                    "Start is not at char boundary"
                );
                let end = pos + c.len_utf8();
                assert!(
                    text.borrow().is_char_boundary(end),
                    "End is not at char boundary"
                );

                line_ranges.push(start..end);
                line_start = None;
            }
        }

        // Handle a situation when there's no newline at the end
        if let (Some(start), Some((pos, c))) = (line_start, last_char) {
            line_ranges.push(start..(pos + c.len_utf8()))
        }

        // Insert an artificial blank line with an empty range
        if let Some((pos, c)) = last_char {
            line_ranges.push(pos + c.len_utf8()..pos + c.len_utf8())
        }

        OffsetMap { text, line_ranges }
    }

    pub fn offset_to_line(&self, offset: usize) -> Option<usize> {
        if offset > self.text.borrow().len() {
            return None;
        }

        for (idx, range) in self.line_ranges.iter().enumerate() {
            if offset >= range.start && offset < range.end {
                return Some(idx);
            }
        }
        panic!("Couldn't translate u8 offset {} to line", offset)
    }

    pub fn offset_to_lsp_position(&self, offset: usize) -> Option<Position> {
        let text = self.text.borrow();

        let line = self.offset_to_line(offset)?;
        let line_range = self.line_ranges[line].clone();

        let u8_offset = offset - line_range.start;
        let mut u16_offset = 0;
        let mut found = false;

        // Handle the case of artificial blank line
        if u8_offset == 0 {
            found = true;
        }

        for (c_off, c) in text[line_range].char_indices() {
            if c_off == u8_offset {
                found = true;
                break;
            } else {
                u16_offset += c.len_utf16();
            }
        }

        assert!(found, "Offset not found in line");
        Some(Position::new(line as u32, u16_offset as u32))
    }

    pub fn range_to_lsp_range(&self, span: &Range<usize>) -> Option<lsp_types::Range> {
        let start = self.offset_to_lsp_position(span.start)?;
        let end = self.offset_to_lsp_position(span.end)?;
        Some(lsp_types::Range::new(start, end))
    }

    pub fn lsp_pos_to_offset(&self, lsp_pos: Position) -> Option<usize> {
        let line_range = self.line_ranges.get(lsp_pos.line as usize)?.to_owned();

        let mut u8_offset = line_range.start;
        let mut u16_offset = 0;
        let mut found = false;

        // Handle the case of artificial blank line
        if u16_offset == lsp_pos.character {
            found = true;
        }

        for c in self.text.borrow()[line_range].chars() {
            if u16_offset == lsp_pos.character {
                found = true;
                break;
            } else {
                u16_offset += c.len_utf16() as u32;
                u8_offset += c.len_utf8();
            }
        }

        assert!(found, "LSP pos not found in line");
        Some(u8_offset)
    }
}

pub fn apply_change<S: Borrow<str>>(
    orig: &str,
    orig_map: &OffsetMap<S>,
    range: Option<lsp_types::Range>,
    patch: &str,
) -> String {
    match range {
        None => patch.to_string(),
        Some(range) => {
            let start_offset = orig_map.lsp_pos_to_offset(range.start).unwrap();
            let end_offset = orig_map.lsp_pos_to_offset(range.end).unwrap();

            let mut new = orig.to_string();
            new.replace_range(start_offset..end_offset, &patch);
            new
        }
    }
}

pub fn text_matches_query(text: &str, query: &str) -> bool {
    let text = text.to_lowercase();
    let query = query.to_lowercase();

    let mut start = 0;
    for c in query.chars() {
        let char_pos = text[start..].find(c);
        start = match char_pos {
            Some(pos) => start + pos + c.len_utf8(),
            _ => return false,
        };
    }

    return true;
}

#[cfg(test)]
mod tests {
    use super::*;

    mod offset_map {
        use super::*;

        #[test]
        fn no_newline() {
            //          012
            let text = "Hi!";
            let offsets = OffsetMap::new(text);
            assert_eq!(offsets.line_ranges, vec![0..3, 3..3]);
        }

        #[test]
        fn newline() {
            //          012 3
            let text = "Hi!\n";
            let offsets = OffsetMap::new(text);
            assert_eq!(offsets.line_ranges, vec![0..4, 4..4]);
        }

        #[test]
        fn win_newline() {
            //          012 3 4
            let text = "Hi!\r\n";
            let offsets = OffsetMap::new(text);
            assert_eq!(offsets.line_ranges, vec![0..5, 5..5]);
        }

        #[test]
        fn two_lines() {
            //          012 345678
            let text = "Hi!\nWorld";
            let offsets = OffsetMap::new(text);
            assert_eq!(offsets.line_ranges, vec![0..4, 4..9, 9..9]);
        }
    }

    #[test]
    fn test_text_matches_query() {
        let text = "Hello World!";
        assert!(text_matches_query(&text, "h"));
        assert!(text_matches_query(&text, "hel"));
        assert!(text_matches_query(&text, "hw"));
        assert!(text_matches_query(&text, "h!"));
        assert!(!text_matches_query(&text, "hz"));
    }

    mod apply_change {
        use super::*;
        #[test]
        fn within_line() {
            let text = "# Hello World";
            let replaced = apply_change(
                text,
                &OffsetMap::new(text),
                Some(lsp_types::Range::new(
                    lsp_types::Position::new(0, 2),
                    lsp_types::Position::new(0, 7),
                )),
                "Hi",
            );
            assert_eq!(&replaced, "# Hi World");
        }

        #[test]
        fn at_newline() {
            //          01 2
            let text = "Hi\n";
            let replaced = apply_change(
                text,
                &OffsetMap::new(text),
                Some(lsp_types::Range::new(
                    lsp_types::Position::new(0, 2),
                    lsp_types::Position::new(1, 0),
                )),
                "",
            );
            assert_eq!(&replaced, "Hi");
        }
    }
}
