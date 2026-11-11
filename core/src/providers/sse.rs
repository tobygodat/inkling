//! Server-sent events, as all three providers stream them.

use std::io::{self, BufRead};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Event {
    pub name: Option<String>,
    pub data: String,
}

/// The next event, or `None` at the end of the stream. Comments and unknown fields are skipped.
pub fn next(reader: &mut impl BufRead) -> io::Result<Option<Event>> {
    let mut event = Event::default();
    let mut has_data = false;
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            return Ok(has_data.then_some(event));
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\n', '\r']);
        if text.is_empty() {
            if has_data {
                return Ok(Some(event));
            }
            event.name = None;
            continue;
        }
        let (field, value) = text.split_once(':').unwrap_or((text, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => event.name = Some(value.to_string()),
            "data" => {
                if has_data {
                    event.data.push('\n');
                }
                event.data.push_str(value);
                has_data = true;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn all(bytes: &str) -> Vec<Event> {
        let mut reader = Cursor::new(bytes.as_bytes());
        std::iter::from_fn(|| next(&mut reader).unwrap()).collect()
    }

    #[test]
    fn events_split_on_blank_lines_with_crlf_comments_and_multiline_data() {
        let events = all(": ping\r\nevent: a\r\ndata: 1\r\n\r\ndata: x\ndata:y\n\nid: 7\n\ndata: tail");
        assert_eq!(
            events,
            vec![
                Event { name: Some("a".into()), data: "1".into() },
                Event { name: None, data: "x\ny".into() },
                Event { name: None, data: "tail".into() },
            ]
        );
    }
}
