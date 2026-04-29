//! Provider pricing table for cost estimation in the status bar.
//! Prices are in USD per million tokens (as of early 2026).
//! Update these when providers change pricing.

/// Per-million-token price for input and output.
#[derive(Debug, Clone, Copy)]
pub struct TokenPrice {
    pub input_per_m: f64,
    pub output_per_m: f64,
}

impl TokenPrice {
    const fn new(input_per_m: f64, output_per_m: f64) -> Self {
        Self { input_per_m, output_per_m }
    }

    /// Compute cost in USD for the given token counts.
    pub fn cost_usd(&self, input_tokens: u64, output_tokens: u64) -> f64 {
        (input_tokens as f64 * self.input_per_m / 1_000_000.0)
            + (output_tokens as f64 * self.output_per_m / 1_000_000.0)
    }
}

/// Look up pricing for a model by name (prefix-matched).
/// Returns `None` for unknown models.
pub fn price_for_model(model: &str) -> Option<TokenPrice> {
    let m = model.to_lowercase();

    // xAI / Grok
    if m.contains("grok-3-mini-fast") {
        return Some(TokenPrice::new(0.30, 0.50));
    }
    if m.contains("grok-3-mini") {
        return Some(TokenPrice::new(0.30, 0.50));
    }
    if m.contains("grok-3-fast") {
        return Some(TokenPrice::new(3.00, 15.00));
    }
    if m.contains("grok-3") {
        return Some(TokenPrice::new(3.00, 15.00));
    }
    if m.contains("grok-4") {
        return Some(TokenPrice::new(3.00, 15.00));
    }

    // Anthropic / Claude
    if m.contains("claude-opus") {
        return Some(TokenPrice::new(15.00, 75.00));
    }
    if m.contains("claude-sonnet") {
        return Some(TokenPrice::new(3.00, 15.00));
    }
    if m.contains("claude-haiku") {
        return Some(TokenPrice::new(0.25, 1.25));
    }

    // OpenAI
    if m.contains("gpt-5") || m.contains("o3") || m.contains("o4") {
        return Some(TokenPrice::new(10.00, 40.00));
    }
    if m.contains("gpt-4") {
        return Some(TokenPrice::new(2.50, 10.00));
    }
    if m.contains("gpt-3.5") {
        return Some(TokenPrice::new(0.50, 1.50));
    }

    // Ollama / local: free
    if m.contains("ollama") || m.contains("qwen") || m.contains("llama") || m.contains("mistral") {
        return Some(TokenPrice::new(0.0, 0.0));
    }

    None
}

/// Format a cost estimate for display: "$0.18" or "<$0.01".
pub fn format_cost(usd: f64) -> String {
    if usd < 0.001 {
        "$0.00".to_string()
    } else if usd < 0.01 {
        format!("${:.3}", usd)
    } else {
        format!("${:.2}", usd)
    }
}

/// Format token counts compactly: "12.3k" or "850".
pub fn format_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grok_fast_price() {
        let p = price_for_model("grok-3-fast").unwrap();
        let cost = p.cost_usd(10_000, 2_000);
        assert!(cost > 0.0);
    }
}
