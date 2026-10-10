//! One-shot prompt shaping (e.g. image path annotation) for CLI `Run` / positional prompt.

use anyhow::Result;

/// If an image path is provided, attach it to the prompt text as a note.
/// The actual image content is embedded in the message when the provider supports it.
pub fn build_prompt_with_image(prompt: &str, image: Option<&std::path::Path>) -> Result<String> {
    match image {
        None => Ok(prompt.to_string()),
        Some(path) => {
            anyhow::ensure!(
                path.is_file(),
                "image file does not exist: {}",
                path.display()
            );
            Ok(format!("{prompt}\n\n[image attached: {}]", path.display()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_without_image_is_unchanged() {
        assert_eq!(
            build_prompt_with_image("hello world", None).unwrap(),
            "hello world"
        );
    }

    #[test]
    fn multipart_images_preserve_bytes_and_reject_invalid_inputs() {
        use harness_provider_core::MessageContent;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preview.PNG");
        std::fs::write(&path, b"image-bytes").unwrap();
        let content = MessageContent::with_image("Review preview", path.to_str().unwrap()).unwrap();
        let value = serde_json::to_value(content).unwrap();
        assert_eq!(value[0]["text"], "Review preview");
        assert_eq!(
            value[1]["image_url"]["url"],
            "data:image/png;base64,aW1hZ2UtYnl0ZXM="
        );
        std::fs::write(&path, b"").unwrap();
        assert!(MessageContent::with_image("Review", path.to_str().unwrap()).is_err());
        std::fs::File::create(&path)
            .unwrap()
            .set_len(10 * 1024 * 1024 + 1)
            .unwrap();
        assert!(MessageContent::with_image("Review", path.to_str().unwrap()).is_err());
        let unsupported = dir.path().join("notes.txt");
        std::fs::write(&unsupported, b"text").unwrap();
        assert!(MessageContent::with_image("Review", unsupported.to_str().unwrap()).is_err());
    }
}
