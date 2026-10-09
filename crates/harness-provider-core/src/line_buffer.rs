//! Decode newline-delimited protocols after complete UTF-8 lines arrive.

use crate::ProviderError;

/// Buffers raw transport bytes, which may split a Unicode character at any point.
#[derive(Default)]
pub struct LineBuffer {
    bytes: Vec<u8>,
}

impl LineBuffer {
    /// Append a transport chunk without attempting to decode partial characters.
    pub fn push(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    /// Decode the next complete line, excluding its newline and optional CR.
    pub fn next_line(&mut self) -> Result<Option<String>, ProviderError> {
        let Some(end) = self.bytes.iter().position(|byte| *byte == b'\n') else {
            return Ok(None);
        };
        let line: Vec<u8> = self.bytes.drain(..=end).collect();
        let text = std::str::from_utf8(&line).map_err(|error| {
            ProviderError::Other(format!("invalid UTF-8 in provider stream: {error}"))
        })?;
        Ok(Some(text.trim_end_matches(['\r', '\n']).to_owned()))
    }

    /// Treat remaining bytes as a final line when a protocol allows no trailing newline.
    pub fn finish(&mut self) {
        if !self.bytes.is_empty() && self.bytes.last() != Some(&b'\n') {
            self.bytes.push(b'\n');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_every_possible_split_of_multilingual_lines() {
        let input = "data: hé中🦀\r\nnext\n";
        for split in 0..=input.len() {
            let mut buffer = LineBuffer::default();
            buffer.push(&input.as_bytes()[..split]);
            let mut lines = Vec::new();
            while let Some(line) = buffer.next_line().unwrap() {
                lines.push(line);
            }
            buffer.push(&input.as_bytes()[split..]);
            while let Some(line) = buffer.next_line().unwrap() {
                lines.push(line);
            }
            assert_eq!(lines, ["data: hé中🦀", "next"]);
        }
    }

    #[test]
    fn invalid_utf8_is_an_error_and_final_line_is_supported() {
        let mut buffer = LineBuffer::default();
        buffer.push(&[0xff, b'\n']);
        assert!(buffer.next_line().is_err());
        buffer.push("é".as_bytes());
        assert!(buffer.next_line().unwrap().is_none());
        buffer.finish();
        assert_eq!(buffer.next_line().unwrap().as_deref(), Some("é"));
    }
}
