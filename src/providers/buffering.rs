//! Byte-oriented buffering for line-delimited provider responses.
//!
//! Converting each network chunk with `from_utf8_lossy` corrupts characters
//! whose bytes are split between chunks. Buffer bytes until a complete line is
//! available, then decode that complete line as UTF-8 instead.

use anyhow::{anyhow, Result};

#[derive(Default)]
pub(crate) struct LineBuffer {
    bytes: Vec<u8>,
}

impl LineBuffer {
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>> {
        self.bytes.extend_from_slice(chunk);
        let mut lines = Vec::new();

        while let Some(newline) = self.bytes.iter().position(|b| *b == b'\n') {
            let mut line: Vec<u8> = self.bytes.drain(..=newline).collect();
            line.pop(); // newline
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            lines.push(
                String::from_utf8(line)
                    .map_err(|_| anyhow!("Provider returned invalid UTF-8 in a streamed line"))?,
            );
        }

        Ok(lines)
    }

    /// Decode a final line when the response ends without a newline.
    pub(crate) fn finish(&mut self) -> Result<Option<String>> {
        if self.bytes.is_empty() {
            return Ok(None);
        }
        let mut line = std::mem::take(&mut self.bytes);
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        String::from_utf8(line)
            .map(Some)
            .map_err(|_| anyhow!("Provider returned invalid UTF-8 at the end of the stream"))
    }
}

/// Incremental SSE event decoder. SSE permits a JSON payload to be split over
/// several `data:` lines; an empty line dispatches the assembled event.
#[derive(Default)]
pub(crate) struct SseDecoder {
    data: String,
}

impl SseDecoder {
    pub(crate) fn push_line(&mut self, line: &str) -> Option<String> {
        if line.is_empty() {
            if self.data.is_empty() {
                return None;
            }
            let event = std::mem::take(&mut self.data);
            // Remove the separator inserted before the next data line.
            return Some(event.strip_suffix('\n').unwrap_or(&event).to_string());
        }
        if line.starts_with(':') {
            return None; // keep-alive/comment
        }
        if let Some(value) = line.strip_prefix("data:") {
            self.data.push_str(value.strip_prefix(' ').unwrap_or(value));
            self.data.push('\n');
        }
        // `event:`, `id:`, `retry:`, and unknown fields do not carry content.
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_utf8_lines_are_not_corrupted() {
        let input = "{\"content\":\"你好 🦀 café\"}\n{\"content\":\"done\"}\n";
        let mut buffer = LineBuffer::default();
        let mut lines = Vec::new();

        // Deliberately split at every byte, including inside multi-byte chars.
        for byte in input.as_bytes() {
            lines.extend(buffer.push(&[*byte]).unwrap());
        }
        lines.extend(buffer.finish().unwrap());

        assert_eq!(
            lines,
            vec!["{\"content\":\"你好 🦀 café\"}", "{\"content\":\"done\"}"]
        );
    }

    #[test]
    fn trailing_line_and_crlf_are_decoded() {
        let mut buffer = LineBuffer::default();
        assert!(buffer.push("héllo\r\nworld".as_bytes()).unwrap()[0] == "héllo");
        assert_eq!(buffer.finish().unwrap().as_deref(), Some("world"));
        assert_eq!(buffer.finish().unwrap(), None);
    }

    #[test]
    fn invalid_utf8_is_an_error_not_replacement_text() {
        let mut buffer = LineBuffer::default();
        assert!(buffer.push(&[0xff, b'\n']).is_err());
    }

    #[test]
    fn sse_multiline_events_and_comments_are_buffered() {
        let mut decoder = SseDecoder::default();
        assert_eq!(decoder.push_line(": keep-alive"), None);
        assert_eq!(decoder.push_line("event: message"), None);
        assert_eq!(decoder.push_line("data: {\"a\":"), None);
        assert_eq!(decoder.push_line("data: 1}"), None);
        assert_eq!(decoder.push_line(""), Some("{\"a\":\n1}".to_string()));

        assert_eq!(decoder.push_line("data: tail"), None);
        assert_eq!(decoder.push_line(""), Some("tail".to_string()));
        assert_eq!(decoder.push_line(""), None);
    }
}
