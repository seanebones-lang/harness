//! One-shot prompt shaping (e.g. image path annotation) for CLI `Run` / positional prompt.

use anyhow::Result;

/// If an image path is provided, attach it to the prompt text as a note.
/// The actual image content is embedded in the message when the provider supports it.
pub fn build_prompt_with_image(prompt: &str, image: Option<&std::path::Path>) -> Result<String> {
    match image {
        None => Ok(prompt.to_string()),
        Some(path) => {
            let _content =
                harness_provider_core::MessageContent::with_image(prompt, &path.to_string_lossy())?;
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
}
