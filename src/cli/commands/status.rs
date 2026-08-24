//! `harness status` — config + MCP + recent sessions summary.

use crate::config::Config;
use anyhow::Result;
use harness_memory::SessionStore;

pub fn run_status(cfg: &Config, model: &str, store: &SessionStore) -> Result<()> {
    println!("harness status\n");

    let route: Vec<String> = cfg
        .router
        .default
        .iter()
        .cloned()
        .chain(cfg.router.fallback.clone().unwrap_or_default())
        .collect();
    println!(
        "  Route   : {}",
        if route.is_empty() {
            "not configured".to_string()
        } else {
            route.join(" → ")
        }
    );
    println!("  Model   : {model}");
    println!("  Access  : {}", selected_access_status(cfg));

    let cfg_path = {
        let local = std::path::PathBuf::from(".harness/config.toml");
        let global = dirs::home_dir()
            .unwrap_or_default()
            .join(".harness/config.toml");
        if local.exists() {
            format!("{} (project)", local.display())
        } else if global.exists() {
            format!("{} (global)", global.display())
        } else {
            "defaults (no config file found)".to_string()
        }
    };
    println!("  Config  : {cfg_path}");

    let mcp_path = harness_mcp::find_config();
    println!(
        "  MCP     : {}",
        mcp_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "not configured".to_string())
    );

    println!();
    println!("Recent sessions:");
    match store.list(5) {
        Ok(sessions) if !sessions.is_empty() => {
            for (id, name, updated) in sessions {
                let short = id.chars().take(8).collect::<String>();
                println!(
                    "  {} · {} · {}",
                    short,
                    name.unwrap_or_else(|| "(unnamed)".to_string()),
                    updated
                );
            }
        }
        Ok(_) => println!("  (none yet)"),
        Err(e) => println!("  (error reading sessions: {e})"),
    }

    println!();
    println!("Run `harness` to start a new session.");
    Ok(())
}

fn selected_access_status(cfg: &Config) -> String {
    let Some(name) = cfg.router.default.as_deref() else {
        return "route not configured (run `harness setup`)".to_string();
    };
    let entry = cfg.providers.get(name);
    if harness_provider_router::provider_preset(name).is_some_and(|preset| preset.local) {
        return "local runtime selected".to_string();
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
        || (name == "xai"
            && cfg
                .provider
                .api_key
                .as_deref()
                .is_some_and(|value| !value.is_empty()))
    {
        return "credential stored in config".to_string();
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
            format!("{env_name} detected")
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
        None => "credential requirements depend on provider runtime".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_status_requires_a_selected_route() {
        assert!(selected_access_status(&Config::default()).contains("route not configured"));
    }

    #[test]
    fn access_status_describes_unauthenticated_custom_endpoint() {
        let mut cfg = Config::default();
        cfg.router.default = Some("local-dev".into());
        cfg.providers.insert(
            "local-dev".into(),
            harness_provider_router::ProviderEntry {
                kind: Some("openai-compatible".into()),
                base_url: Some("http://127.0.0.1:1234/v1".into()),
                model: Some("local-model".into()),
                ..harness_provider_router::ProviderEntry::default()
            },
        );
        assert_eq!(selected_access_status(&cfg), "no bearer token configured");
    }
}
