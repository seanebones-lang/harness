//! Provider pricing table for cost estimation in the status bar.
//! Prices are in USD per million tokens (April 2026).
//! Update these when providers change pricing.

/// Per-million-token price for input, cached input, and output.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct TokenPrice {
    pub input_per_m: f64,
    /// Price for cache-hit tokens (prompt caching). 0.0 means no caching support.
    pub cached_input_per_m: f64,
    pub output_per_m: f64,
}

impl TokenPrice {
    const fn new(input_per_m: f64, cached_input_per_m: f64, output_per_m: f64) -> Self {
        Self {
            input_per_m,
            cached_input_per_m,
            output_per_m,
        }
    }

    /// Compute cost in USD for standard (non-cached) token counts.
    pub fn cost_usd(&self, input_tokens: u64, output_tokens: u64) -> f64 {
        (input_tokens as f64 * self.input_per_m / 1_000_000.0)
            + (output_tokens as f64 * self.output_per_m / 1_000_000.0)
    }

    /// Compute cost in USD factoring in prompt-cache hits.
    #[allow(dead_code)]
    pub fn cost_with_cache(
        &self,
        input_tokens: u64,
        cached_tokens: u64,
        output_tokens: u64,
    ) -> f64 {
        let cache_rate = if self.cached_input_per_m > 0.0 {
            self.cached_input_per_m
        } else {
            self.input_per_m
        };
        (input_tokens as f64 * self.input_per_m / 1_000_000.0)
            + (cached_tokens as f64 * cache_rate / 1_000_000.0)
            + (output_tokens as f64 * self.output_per_m / 1_000_000.0)
    }
}

/// Look up pricing for a model by name (prefix-matched).
/// Returns `None` for unknown models.
pub fn price_for_model(model: &str) -> Option<TokenPrice> {
    let m = model.to_lowercase();

    // xAI / Grok — April 2026 SKUs
    // Grok 4.20: $2/$6, cached input $0.20 (90% off)
    if m.contains("grok-4.20") || m.contains("grok-4-20") {
        return Some(TokenPrice::new(2.00, 0.20, 6.00));
    }
    // Grok 4.1 Fast: $0.20/$0.50, cached $0.05
    if m.contains("grok-4-1-fast") || m.contains("grok-4.1-fast") {
        return Some(TokenPrice::new(0.20, 0.05, 0.50));
    }
    // Grok 4 flagship: $3/$15
    if m.contains("grok-4") {
        return Some(TokenPrice::new(3.00, 0.0, 15.00));
    }
    // Grok 3 legacy: $2/$10 (kept for backward compat)
    if m.contains("grok-3-mini") {
        return Some(TokenPrice::new(0.30, 0.0, 0.50));
    }
    if m.contains("grok-3") {
        return Some(TokenPrice::new(2.00, 0.0, 10.00));
    }

    // Anthropic / Claude — April 2026 SKUs
    // Opus 4.7 / 4.6 / 4.5: $5/$25, cached reads $0.50
    if m.contains("claude-opus-4-7")
        || m.contains("claude-opus-4-6")
        || m.contains("claude-opus-4-5")
    {
        return Some(TokenPrice::new(5.00, 0.50, 25.00));
    }
    // Opus 4.1 / 4 (legacy): $15/$75
    if m.contains("claude-opus-4-1")
        || m.contains("claude-opus-4-0")
        || m.contains("claude-opus-4-20250514")
    {
        return Some(TokenPrice::new(15.00, 1.50, 75.00));
    }
    // Opus 3 (deprecated): $15/$75
    if m.contains("claude-opus") {
        return Some(TokenPrice::new(15.00, 1.50, 75.00));
    }
    // Sonnet 4.x: $3/$15, cached reads $0.30
    if m.contains("claude-sonnet") {
        return Some(TokenPrice::new(3.00, 0.30, 15.00));
    }
    // Haiku 4.5: $1/$5, cached reads $0.10
    if m.contains("claude-haiku-4-5") {
        return Some(TokenPrice::new(1.00, 0.10, 5.00));
    }
    // Haiku 3.5: $0.80/$4, cached $0.08
    if m.contains("claude-haiku-3-5") {
        return Some(TokenPrice::new(0.80, 0.08, 4.00));
    }
    // Haiku 3: $0.25/$1.25
    if m.contains("claude-haiku") {
        return Some(TokenPrice::new(0.25, 0.0, 1.25));
    }

    // OpenAI — April 2026 SKUs
    // GPT-5.5: $5/$30, cached $0.50
    if m.contains("gpt-5.5") {
        return Some(TokenPrice::new(5.00, 0.50, 30.00));
    }
    // GPT-5.4: $2.50/$15, cached $0.25
    if m.contains("gpt-5.4-nano") {
        return Some(TokenPrice::new(0.20, 0.02, 1.25));
    }
    if m.contains("gpt-5.4-mini") {
        return Some(TokenPrice::new(0.75, 0.075, 4.50));
    }
    if m.contains("gpt-5.4") {
        return Some(TokenPrice::new(2.50, 0.25, 15.00));
    }
    // GPT-5 / GPT-5.x legacy
    if m.contains("gpt-5") {
        return Some(TokenPrice::new(1.25, 0.125, 10.00));
    }
    // o4-mini: $1.10/$4.40
    if m.contains("o4-mini") {
        return Some(TokenPrice::new(1.10, 0.275, 4.40));
    }
    // o3: $1/$4
    if m.contains("o3") {
        return Some(TokenPrice::new(1.00, 0.25, 4.00));
    }
    // GPT-4o legacy
    if m.contains("gpt-4o") {
        return Some(TokenPrice::new(2.50, 1.25, 10.00));
    }
    if m.contains("gpt-4") {
        return Some(TokenPrice::new(2.50, 0.0, 10.00));
    }

    // Ollama / local: free
    if m.contains("qwen")
        || m.contains("llama")
        || m.contains("mistral")
        || m.contains("deepseek")
        || m.contains("gemma")
        || m.contains("phi")
        || m.contains("nomic")
        || m.contains("ollama")
    {
        return Some(TokenPrice::new(0.0, 0.0, 0.0));
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
    fn grok_4_20_price() {
        let p = price_for_model("grok-4.20-0309-reasoning").unwrap();
        let cost = p.cost_usd(10_000, 2_000);
        assert!(cost > 0.0);
    }

    #[test]
    fn claude_opus_47_price() {
        let p = price_for_model("claude-opus-4-7").unwrap();
        assert!((p.input_per_m - 5.00).abs() < 0.01);
        assert!((p.output_per_m - 25.00).abs() < 0.01);
        assert!((p.cached_input_per_m - 0.50).abs() < 0.01);
    }

    #[test]
    fn gpt55_price() {
        let p = price_for_model("gpt-5.5").unwrap();
        assert!((p.input_per_m - 5.00).abs() < 0.01);
        assert!((p.output_per_m - 30.00).abs() < 0.01);
    }

    #[test]
    fn cost_with_cache_cheaper() {
        let p = price_for_model("claude-sonnet-4-6").unwrap();
        let standard = p.cost_usd(10_000, 2_000);
        // 8k from cache, 2k fresh input, 2k output
        let cached = p.cost_with_cache(2_000, 8_000, 2_000);
        assert!(cached < standard);
    }
}
