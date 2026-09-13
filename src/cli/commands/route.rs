//! `harness route` — provider-neutral route configuration.

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::cli::RouteAction;
use crate::config::{self, Config};

pub fn handle_route_command(
    action: &RouteAction,
    loaded_cfg: &Config,
    explicit_config: Option<&Path>,
) -> Result<()> {
    match action {
        RouteAction::Show { global, project } => {
            let path = route_config_path(*global, *project, explicit_config)?;
            let cfg = load_for_path(&path, loaded_cfg)?;
            show_route(&path, &cfg);
        }
        RouteAction::Set {
            route,
            global,
            project,
        } => {
            let path = route_config_path(*global, *project, explicit_config)?;
            let mut cfg = load_for_path(&path, loaded_cfg)?;
            set_route(&mut cfg, route)?;
            save_and_show(&path, &cfg)?;
        }
        RouteAction::Model {
            provider,
            model,
            global,
            project,
        } => {
            let path = route_config_path(*global, *project, explicit_config)?;
            let mut cfg = load_for_path(&path, loaded_cfg)?;
            set_model(&mut cfg, provider, model)?;
            save_and_show(&path, &cfg)?;
        }
        RouteAction::Add {
            provider,
            position,
            global,
            project,
        } => {
            let path = route_config_path(*global, *project, explicit_config)?;
            let mut cfg = load_for_path(&path, loaded_cfg)?;
            add_to_route(&mut cfg, provider, *position)?;
            save_and_show(&path, &cfg)?;
        }
        RouteAction::Remove {
            provider,
            global,
            project,
        } => {
            let path = route_config_path(*global, *project, explicit_config)?;
            let mut cfg = load_for_path(&path, loaded_cfg)?;
            remove_from_route(&mut cfg, provider)?;
            save_and_show(&path, &cfg)?;
        }
        RouteAction::Move {
            provider,
            position,
            global,
            project,
        } => {
            let path = route_config_path(*global, *project, explicit_config)?;
            let mut cfg = load_for_path(&path, loaded_cfg)?;
            move_in_route(&mut cfg, provider, *position)?;
            save_and_show(&path, &cfg)?;
        }
        RouteAction::Custom {
            name,
            base_url,
            model,
            api_key_env,
            add,
            global,
            project,
        } => {
            let path = route_config_path(*global, *project, explicit_config)?;
            let mut cfg = load_for_path(&path, loaded_cfg)?;
            configure_custom_provider(&mut cfg, name, base_url, model, api_key_env, *add)?;
            save_and_show(&path, &cfg)?;
        }
    }
    Ok(())
}

fn route_config_path(
    global: bool,
    project: bool,
    explicit_config: Option<&Path>,
) -> Result<PathBuf> {
    if global {
        return Ok(dirs::home_dir()
            .context("cannot determine home directory")?
            .join(".harness/config.toml"));
    }
    if project {
        return Ok(PathBuf::from(".harness/config.toml"));
    }
    Ok(explicit_config
        .map(Path::to_path_buf)
        .unwrap_or_else(config::active_config_toml_path))
}

fn load_for_path(path: &Path, loaded_cfg: &Config) -> Result<Config> {
    if path.exists() {
        config::load(Some(path))
            .with_context(|| format!("failed to load route config from {}", path.display()))
    } else if path == config::active_config_toml_path() {
        Ok(loaded_cfg.clone())
    } else {
        Ok(Config::default())
    }
}

fn save_and_show(path: &Path, cfg: &Config) -> Result<()> {
    config::write_config_toml(path, cfg)
        .with_context(|| format!("failed to write route config to {}", path.display()))?;
    println!("Saved {}", path.display());
    show_route(path, cfg);
    Ok(())
}

fn route_names(cfg: &Config) -> Vec<String> {
    cfg.router
        .default
        .iter()
        .cloned()
        .chain(cfg.router.fallback.clone().unwrap_or_default())
        .collect()
}

fn write_route_names(cfg: &mut Config, route: &[String]) -> Result<()> {
    let Some(primary) = route.first() else {
        anyhow::bail!("a route must contain at least one provider");
    };
    cfg.router.default = Some(primary.clone());
    cfg.router.fallback = Some(route[1..].to_vec());
    cfg.provider.model = cfg
        .providers
        .get(primary)
        .and_then(|entry| entry.model.clone());
    Ok(())
}

fn parse_provider_model(spec: &str) -> Result<(String, String)> {
    let (provider, model) = spec
        .trim()
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("'{spec}' must use provider:model format"))?;
    let provider = normalize_provider_name(provider)?;
    let model = model.trim();
    if model.is_empty() {
        anyhow::bail!("'{spec}' is missing a model after ':'");
    }
    Ok((provider, model.to_string()))
}

fn normalize_provider_name(name: &str) -> Result<String> {
    let name = name.trim().to_ascii_lowercase();
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        anyhow::bail!("provider names may contain only lowercase letters, numbers, '-' and '_'");
    }
    Ok(name)
}

fn ensure_provider<'a>(
    cfg: &'a mut Config,
    name: &str,
) -> Result<&'a mut harness_provider_router::ProviderEntry> {
    if !cfg.providers.contains_key(name) {
        if harness_provider_router::provider_preset(name).is_none() {
            anyhow::bail!(
                "unknown provider '{name}'; configure it first with `harness route custom {name} --base-url <URL> --model <MODEL> --api-key-env <ENV>`"
            );
        }
        cfg.providers.insert(
            name.to_string(),
            harness_provider_router::configured_provider_entry(name),
        );
    }
    cfg.providers
        .get_mut(name)
        .context("provider entry was not created")
}

pub(crate) fn set_route(cfg: &mut Config, specs: &[String]) -> Result<()> {
    let parsed: Vec<(String, String)> = specs
        .iter()
        .map(|spec| parse_provider_model(spec))
        .collect::<Result<_>>()?;
    let mut seen = HashSet::new();
    for (provider, _) in &parsed {
        if !seen.insert(provider.clone()) {
            anyhow::bail!("provider route contains duplicate entry '{provider}'");
        }
    }
    for (provider, model) in &parsed {
        ensure_provider(cfg, provider)?.model = Some(model.clone());
    }
    let names: Vec<String> = parsed.into_iter().map(|(provider, _)| provider).collect();
    write_route_names(cfg, &names)
}

fn set_model(cfg: &mut Config, provider: &str, model: &str) -> Result<()> {
    let provider = normalize_provider_name(provider)?;
    let model = model.trim();
    if model.is_empty() {
        anyhow::bail!("model is required");
    }
    if !route_names(cfg).contains(&provider) {
        anyhow::bail!("provider '{provider}' is not in the saved route");
    }
    ensure_provider(cfg, &provider)?.model = Some(model.to_string());
    if cfg.router.default.as_deref() == Some(provider.as_str()) {
        cfg.provider.model = Some(model.to_string());
    }
    Ok(())
}

fn add_to_route(cfg: &mut Config, spec: &str, position: Option<usize>) -> Result<()> {
    let (provider, model) = parse_provider_model(spec)?;
    let mut route = route_names(cfg);
    if route.contains(&provider) {
        anyhow::bail!("provider '{provider}' is already in the saved route");
    }
    ensure_provider(cfg, &provider)?.model = Some(model);
    let index = match position {
        Some(0) => anyhow::bail!("position is one-based and must be at least 1"),
        Some(value) if value > route.len() + 1 => {
            anyhow::bail!("position {value} exceeds route length {}", route.len() + 1)
        }
        Some(value) => value - 1,
        None => route.len(),
    };
    route.insert(index, provider);
    write_route_names(cfg, &route)
}

fn remove_from_route(cfg: &mut Config, provider: &str) -> Result<()> {
    let provider = normalize_provider_name(provider)?;
    let mut route = route_names(cfg);
    let Some(index) = route.iter().position(|name| name == &provider) else {
        anyhow::bail!("provider '{provider}' is not in the saved route");
    };
    if route.len() == 1 {
        anyhow::bail!(
            "cannot remove the only provider; replace the route with `harness route set`"
        );
    }
    route.remove(index);
    write_route_names(cfg, &route)
}

fn move_in_route(cfg: &mut Config, provider: &str, position: usize) -> Result<()> {
    let provider = normalize_provider_name(provider)?;
    let mut route = route_names(cfg);
    if position == 0 || position > route.len() {
        anyhow::bail!("position must be between 1 and {}", route.len());
    }
    let Some(current) = route.iter().position(|name| name == &provider) else {
        anyhow::bail!("provider '{provider}' is not in the saved route");
    };
    let name = route.remove(current);
    route.insert(position - 1, name);
    write_route_names(cfg, &route)
}

fn configure_custom_provider(
    cfg: &mut Config,
    name: &str,
    base_url: &str,
    model: &str,
    api_key_env: &Option<String>,
    add: bool,
) -> Result<()> {
    let name = normalize_provider_name(name)?;
    let base_url = base_url.trim();
    let model = model.trim();
    if base_url.is_empty() || model.is_empty() {
        anyhow::bail!("base URL and model are required");
    }
    let parsed_url = reqwest::Url::parse(base_url).context("base URL is not a valid URL")?;
    if !matches!(parsed_url.scheme(), "http" | "https") || parsed_url.host_str().is_none() {
        anyhow::bail!("base URL must be an absolute http:// or https:// URL");
    }
    let api_key_env = api_key_env
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(api_key_env) = api_key_env {
        if !api_key_env
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
        {
            anyhow::bail!("API-key environment variable must use A-Z, 0-9, and '_'");
        }
    }
    let entry = cfg.providers.entry(name.clone()).or_default();
    entry.kind = Some("openai-compatible".to_string());
    entry.base_url = Some(base_url.trim_end_matches('/').to_string());
    entry.model = Some(model.to_string());
    entry.api_key_env = api_key_env.map(str::to_string);
    if add && !route_names(cfg).contains(&name) {
        let mut route = route_names(cfg);
        route.push(name);
        write_route_names(cfg, &route)?;
    }
    Ok(())
}

fn show_route(path: &Path, cfg: &Config) {
    println!("Config: {}", path.display());
    let route = route_names(cfg);
    if route.is_empty() {
        println!("Route: not configured");
        println!("Set one with: harness route set <provider:model> [provider:model ...]");
        return;
    }
    println!("Route (exact order):");
    for (index, name) in route.iter().enumerate() {
        let role = if index == 0 { "primary" } else { "fallback" };
        let entry = cfg.providers.get(name);
        let model = entry
            .and_then(|value| value.model.as_deref())
            .unwrap_or("<model not set>");
        let credentials = credential_status(name, entry);
        println!(
            "  {}. {:<10} {}:{} ({credentials})",
            index + 1,
            role,
            name,
            model
        );
    }
}

fn credential_status(name: &str, entry: Option<&harness_provider_router::ProviderEntry>) -> String {
    if harness_provider_router::provider_preset(name).is_some_and(|preset| preset.local) {
        return "local runtime".to_string();
    }
    if name == "bedrock" {
        let env_ready = ["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY"]
            .iter()
            .all(|env_name| std::env::var(env_name).is_ok_and(|value| !value.is_empty()));
        let pair_ready = entry
            .and_then(|value| value.api_key.as_deref())
            .and_then(|value| value.split_once(':'))
            .is_some_and(|(access, secret)| !access.is_empty() && !secret.is_empty());
        return if env_ready || pair_ready {
            "AWS credentials detected".to_string()
        } else {
            "AWS credentials incomplete".to_string()
        };
    }
    if entry
        .and_then(|value| value.api_key.as_deref())
        .is_some_and(|value| !value.is_empty())
    {
        return "key stored in config".to_string();
    }
    let env_name = entry
        .and_then(|value| harness_provider_router::provider_key_environment(name, value))
        .or_else(|| {
            harness_provider_router::provider_preset(name)
                .and_then(|preset| preset.api_key_envs.first())
                .map(|value| (*value).to_string())
        });
    match env_name {
        Some(env_name) if std::env::var(&env_name).is_ok_and(|value| !value.is_empty()) => {
            format!("{env_name} set")
        }
        Some(env_name) => format!("{env_name} not set"),
        None if entry.is_some_and(|value| {
            matches!(
                value.kind.as_deref(),
                Some("openai-compatible" | "compatible")
            )
        }) =>
        {
            "no bearer token configured".to_string()
        }
        None => "credentials depend on provider runtime".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_route_preserves_exact_order_and_models() {
        let mut cfg = Config::default();
        set_route(
            &mut cfg,
            &[
                "groq:llama-3.3-70b-versatile".into(),
                "anthropic:claude-sonnet-4-6".into(),
                "ollama:qwen3-coder:30b".into(),
            ],
        )
        .expect("route");
        assert_eq!(cfg.router.default.as_deref(), Some("groq"));
        assert_eq!(
            cfg.router.fallback.as_deref(),
            Some(&["anthropic".into(), "ollama".into()][..])
        );
        assert_eq!(
            cfg.providers
                .get("ollama")
                .and_then(|entry| entry.model.as_deref()),
            Some("qwen3-coder:30b")
        );
    }

    #[test]
    fn route_mutations_do_not_reorder_other_providers() {
        let mut cfg = Config::default();
        set_route(&mut cfg, &["openai:gpt-a".into(), "xai:grok-b".into()]).expect("set");
        add_to_route(&mut cfg, "ollama:qwen:c", Some(2)).expect("add");
        assert_eq!(route_names(&cfg), ["openai", "ollama", "xai"]);
        move_in_route(&mut cfg, "xai", 1).expect("move");
        assert_eq!(route_names(&cfg), ["xai", "openai", "ollama"]);
        remove_from_route(&mut cfg, "openai").expect("remove");
        assert_eq!(route_names(&cfg), ["xai", "ollama"]);
    }

    #[test]
    fn custom_provider_records_adapter_and_env_without_secret() {
        let mut cfg = Config::default();
        configure_custom_provider(
            &mut cfg,
            "my-cloud",
            "https://example.com/v1/",
            "my-model",
            &Some("MY_CLOUD_API_KEY".into()),
            false,
        )
        .expect("custom");
        let entry = cfg.providers.get("my-cloud").expect("entry");
        assert_eq!(entry.kind.as_deref(), Some("openai-compatible"));
        assert_eq!(entry.base_url.as_deref(), Some("https://example.com/v1"));
        assert_eq!(entry.api_key_env.as_deref(), Some("MY_CLOUD_API_KEY"));
        assert!(entry.api_key.is_none());
    }

    #[test]
    fn custom_local_provider_can_be_unauthenticated() {
        let mut cfg = Config::default();
        configure_custom_provider(
            &mut cfg,
            "local-server",
            "http://127.0.0.1:1234/v1",
            "local-model",
            &None,
            true,
        )
        .expect("custom local");
        let entry = cfg.providers.get("local-server").expect("entry");
        assert!(entry.api_key_env.is_none());
        assert_eq!(cfg.router.default.as_deref(), Some("local-server"));
    }
}

#[cfg(test)]
mod tests_extended {
    use super::*;
    use crate::config::Config;

    #[test]
    fn route_names_returns_empty_for_default_config() {
        let cfg = Config::default();
        assert!(route_names(&cfg).is_empty());
    }

    #[test]
    fn route_names_returns_primary_and_fallbacks() {
        let mut cfg = Config::default();
        cfg.router.default = Some("openai".into());
        cfg.router.fallback = Some(vec!["anthropic".into(), "ollama".into()]);
        assert_eq!(route_names(&cfg), ["openai", "anthropic", "ollama"]);
    }

    #[test]
    fn write_route_names_updates_legacy_provider_model() {
        let mut cfg = Config::default();
        cfg.providers.entry("openai".into()).or_default().model = Some("gpt-4".into());
        write_route_names(&mut cfg, &["openai".into(), "anthropic".into()]).expect("write");
        assert_eq!(cfg.provider.model.as_deref(), Some("gpt-4"));
    }

    #[test]
    fn parse_provider_model_splits_correctly() {
        let (provider, model) = parse_provider_model("openai:gpt-4").expect("parse");
        assert_eq!(provider, "openai");
        assert_eq!(model, "gpt-4");
    }

    #[test]
    fn parse_provider_model_handles_model_with_colons() {
        let (provider, model) = parse_provider_model("ollama:qwen2.5-coder:7b").expect("parse");
        assert_eq!(provider, "ollama");
        assert_eq!(model, "qwen2.5-coder:7b");
    }

    #[test]
    fn parse_provider_model_rejects_no_colon() {
        assert!(parse_provider_model("openai").is_err());
        assert!(parse_provider_model("").is_err());
    }

    #[test]
    fn parse_provider_model_rejects_empty_model() {
        assert!(parse_provider_model("openai:").is_err());
        assert!(parse_provider_model("openai:  ").is_err());
    }

    #[test]
    fn normalize_provider_name_lowercases_and_validates() {
        assert_eq!(normalize_provider_name("OPENAI").expect("norm"), "openai");
        assert_eq!(normalize_provider_name("Anthropic").expect("norm"), "anthropic");
        assert_eq!(normalize_provider_name("my-provider_123").expect("norm"), "my-provider_123");
    }

    #[test]
    fn normalize_provider_name_rejects_invalid_chars() {
        assert!(normalize_provider_name("openai!").is_err());
        assert!(normalize_provider_name("open ai").is_err());
        assert!(normalize_provider_name("").is_err());
    }

    #[test]
    fn ensure_provider_creates_entry_for_known_preset() {
        let mut cfg = Config::default();
        let entry = ensure_provider(&mut cfg, "openai").expect("ensure");
        assert!(entry.model.is_none());
        assert!(cfg.providers.contains_key("openai"));
    }

    #[test]
    fn ensure_provider_rejects_unknown_without_preset() {
        let mut cfg = Config::default();
        assert!(ensure_provider(&mut cfg, "unknown-provider").is_err());
    }

    #[test]
    fn set_route_rejects_duplicate_provider() {
        let mut cfg = Config::default();
        assert!(set_route(&mut cfg, &["openai:gpt".into(), "openai:gpt2".into()]).is_err());
    }

    #[test]
    fn set_model_rejects_provider_not_in_route() {
        let mut cfg = Config::default();
        cfg.router.default = Some("openai".into());
        assert!(set_model(&mut cfg, "anthropic", "claude").is_err());
    }

    #[test]
    fn add_to_route_rejects_existing_provider() {
        let mut cfg = Config::default();
        set_route(&mut cfg, &["openai:gpt".into()]).expect("set");
        assert!(add_to_route(&mut cfg, "openai:gpt2", None).is_err());
    }

    #[test]
    fn add_to_route_position_handles_bounds() {
        let mut cfg = Config::default();
        set_route(&mut cfg, &["openai:gpt".into()]).expect("set");
        assert!(add_to_route(&mut cfg, "anthropic:claude", Some(0)).is_err());
        assert!(add_to_route(&mut cfg, "anthropic:claude", Some(10)).is_err());
        add_to_route(&mut cfg, "anthropic:claude", Some(2)).expect("add end");
        assert_eq!(route_names(&cfg), ["openai", "anthropic"]);
    }

    #[test]
    fn remove_from_route_rejects_only_provider() {
        let mut cfg = Config::default();
        set_route(&mut cfg, &["openai:gpt".into()]).expect("set");
        assert!(remove_from_route(&mut cfg, "openai").is_err());
    }

    #[test]
    fn remove_from_route_removes_provider() {
        let mut cfg = Config::default();
        set_route(&mut cfg, &["openai:gpt".into(), "anthropic:claude".into()]).expect("set");
        remove_from_route(&mut cfg, "openai").expect("remove");
        assert_eq!(route_names(&cfg), ["anthropic"]);
    }

    #[test]
    fn move_in_route_validates_position() {
        let mut cfg = Config::default();
        set_route(&mut cfg, &["openai:gpt".into(), "anthropic:claude".into()]).expect("set");
        assert!(move_in_route(&mut cfg, "openai", 0).is_err());
        assert!(move_in_route(&mut cfg, "openai", 3).is_err());
        move_in_route(&mut cfg, "openai", 2).expect("move");
        assert_eq!(route_names(&cfg), ["anthropic", "openai"]);
    }

    #[test]
    fn configure_custom_provider_validates_url() {
        let mut cfg = Config::default();
        assert!(configure_custom_provider(&mut cfg, "test", "not-a-url", "model", &None, false).is_err());
        assert!(configure_custom_provider(&mut cfg, "test", "ftp://example.com", "model", &None, false).is_err());
        assert!(configure_custom_provider(&mut cfg, "test", "http://", "model", &None, false).is_err());
    }

    #[test]
    fn configure_custom_provider_validates_api_key_env() {
        let mut cfg = Config::default();
        assert!(configure_custom_provider(&mut cfg, "test", "https://example.com", "model", &Some("INVALID-KEY!".into()), false).is_err());
        assert!(configure_custom_provider(&mut cfg, "test", "https://example.com", "model", &Some("VALID_KEY_123".into()), false).is_ok());
    }

    #[test]
    fn configure_custom_provider_trims_trailing_slash() {
        let mut cfg = Config::default();
        configure_custom_provider(&mut cfg, "test", "https://example.com/v1/", "model", &None, false).expect("custom");
        assert_eq!(cfg.providers.get("test").and_then(|e| e.base_url.as_deref()), Some("https://example.com/v1"));
    }

    #[test]
    fn show_route_handles_empty_config() {
        let cfg = Config::default();
        // Just verify it doesn't panic - output goes to stdout
        show_route(std::path::Path::new("/test"), &cfg);
    }

    #[test]
    fn credential_status_local_provider() {
        let mut cfg = Config::default();
        cfg.providers.entry("ollama".into()).or_default();
        let status = credential_status("ollama", cfg.providers.get("ollama"));
        assert_eq!(status, "local runtime");
    }

    #[test]
    fn credential_status_bedrock_env_credentials() {
        std::env::set_var("AWS_ACCESS_KEY_ID", "test");
        std::env::set_var("AWS_SECRET_ACCESS_KEY", "test");
        let mut cfg = Config::default();
        cfg.providers.entry("bedrock".into()).or_default();
        let status = credential_status("bedrock", cfg.providers.get("bedrock"));
        assert_eq!(status, "AWS credentials detected");
        std::env::remove_var("AWS_ACCESS_KEY_ID");
        std::env::remove_var("AWS_SECRET_ACCESS_KEY");
    }
}
