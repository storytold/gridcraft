use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use reqwest::Url;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::thinking::{Levels, Thinking};
use crate::wingman::mcp::ServerConfig;

static SETTINGS_ROOT: OnceLock<PathBuf> = OnceLock::new();
const SETTINGS_LIMIT: u64 = 1024 * 1024;
const EXAMPLE: &str = r#"{
  "language_models": {
    "openai_compatible": {
      "Local": {
        "api_url": "http://127.0.0.1:8000/v1",
        "auto_discover": true,
        "available_models": []
      }
    }
  },
  "wingman": { "agents": {} }
}
"#;

/// The desktop supplies its already validated profile directory, including nightly isolation.
pub fn set_config_dir(dir: PathBuf) {
    let _ = SETTINGS_ROOT.set(dir);
}

fn path() -> Result<PathBuf, String> {
    SETTINGS_ROOT.get().map(|dir| dir.join("wingman.json")).ok_or_else(|| "No desktop settings directory is available.".into())
}

pub(in super::super) fn open_config() -> Result<(), String> {
    let path = path()?;
    let root = path.parent().ok_or("No settings directory is available.")?;
    fs::create_dir_all(root).map_err(|e| format!("Cannot create settings directory: {e}"))?;
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    match options.open(&path) {
        Ok(mut file) => file.write_all(EXAMPLE.as_bytes()).map_err(|e| format!("Cannot write wingman.json: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(format!("Cannot create wingman.json: {e}")),
    }
    open_editor(&path).map_err(|e| format!("Cannot open {}: {e}", path.display()))
}
#[cfg(windows)]
fn open_editor(path: &Path) -> std::io::Result<()> {
    let root = std::env::var_os("SystemRoot").ok_or_else(|| std::io::Error::other("SystemRoot is missing"))?;
    std::process::Command::new(PathBuf::from(root).join("System32/notepad.exe")).arg(path).spawn().map(|_| ())
}
#[cfg(target_os = "macos")]
fn open_editor(path: &Path) -> std::io::Result<()> {
    std::process::Command::new("/usr/bin/open").arg("-t").arg(path).spawn().map(|_| ())
}
#[cfg(not(any(windows, target_os = "macos")))]
fn open_editor(path: &Path) -> std::io::Result<()> {
    open::that_detached(path)
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Config {
    language_models: Models,
    agent: DefaultAgent,
    wingman: Aliases,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct Models {
    openai_compatible: BTreeMap<String, Provider>,
    ollama: Option<Provider>,
    #[serde(alias = "lm_studio")]
    lmstudio: Option<Provider>,
    #[serde(rename = "llama.cpp")]
    llama_cpp: Option<Provider>,
    openai: Option<Provider>,
}
#[derive(Default, Deserialize)]
struct DefaultAgent {
    default_model: Option<ModelRef>,
    default_profile: Option<String>,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct Aliases {
    agents: BTreeMap<String, Alias>,
}
#[derive(Deserialize)]
struct Alias {
    #[serde(flatten)]
    target: ModelRef,
    default_profile: Option<String>,
    #[serde(default)]
    instructions: String,
}
#[derive(Deserialize)]
struct ModelRef {
    provider: String,
    model: String,
    enable_thinking: Option<bool>,
    effort: Option<String>,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct Provider {
    api_url: Option<String>,
    available_models: Vec<Model>,
    custom_headers: BTreeMap<String, String>,
    api_key_env: Option<String>,
    auto_discover: Option<bool>,
    context_window: Option<u32>,
    low_speed_timeout_in_seconds: Option<u64>,
}
#[derive(Default, Clone, Deserialize)]
#[serde(default)]
struct Model {
    name: String,
    display_name: Option<String>,
    max_tokens: Option<u32>,
    max_output_tokens: Option<u32>,
    supports_tools: Option<bool>,
    supports_thinking: Option<bool>,
    reasoning_effort: Option<Levels>,
    reasoning_effort_levels: Option<Levels>,
    capabilities: Caps,
}
#[derive(Default, Clone, Deserialize)]
#[serde(default)]
struct Caps {
    tools: Option<bool>,
    chat_completions: Option<bool>,
    parallel_tool_calls: bool,
    max_tokens_parameter: bool,
    interleaved_reasoning: bool,
}

#[derive(Clone)]
pub(in super::super) struct Agent {
    pub name: String,
    pub provider: String,
    pub model: String,
    pub endpoint: String,
    pub supports_tools: bool,
    pub max_tokens: Option<u32>,
    pub default_profile: Option<String>,
    pub instructions: String,
    pub(super) protocol: Protocol,
    pub(super) max_output_tokens: Option<u32>,
    pub thinking: Thinking,
    pub(super) parallel: bool,
    pub(super) legacy_limit: bool,
    pub(super) reasoning_history: bool,
    pub(super) completions: bool,
    pub(super) headers: HeaderMap,
    pub(super) key_env: Option<String>,
    pub(super) idle_secs: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Protocol {
    Completions,
    Ollama,
}

impl Agent {
    pub(super) fn retain_reasoning(&self) -> bool {
        self.thinking.supports_reasoning_history() && (self.reasoning_history || self.protocol == Protocol::Ollama)
    }

    /// Only a history-sizing hint: the model server owns tokenization. Never truncate the
    /// current user message or silently change the model's configured generation limits.
    pub(in super::super) fn history_budget(&self) -> Option<usize> {
        let context = self.max_tokens.or((self.protocol == Protocol::Ollama).then_some(4096))?;
        let output = self.max_output_tokens.unwrap_or(context / 4);
        Some((context.saturating_sub(output) as usize).saturating_mul(3))
    }
}

#[derive(Clone)]
pub(super) struct Host {
    pub(super) template: Agent,
    pub(super) discover: bool,
}

pub(super) struct Catalog {
    pub(super) hosts: Vec<Host>,
    pub(super) declared: Vec<Agent>,
    preferred: Option<ModelRef>,
}

fn config<T: DeserializeOwned + Default>() -> Result<T, String> {
    let path = path()?;
    let file = match File::open(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        other => other.map_err(|e| format!("Cannot read wingman.json: {e}"))?,
    };
    let mut data = Vec::new();
    file.take(SETTINGS_LIMIT + 1).read_to_end(&mut data).map_err(|e| format!("Cannot read wingman.json: {e}"))?;
    if data.len() as u64 > SETTINGS_LIMIT {
        return Err("wingman.json exceeds 1 MiB.".into());
    }
    serde_json::from_slice(data.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&data))
        .map_err(|e| format!("Invalid wingman.json at line {}, column {}.", e.line(), e.column()))
}

/// Parse MCP settings only when explicitly requested: a broken server definition cannot
/// stop ordinary workbook chat or start a subprocess while opening the panel.
pub(in super::super) fn mcp_servers() -> Result<BTreeMap<String, ServerConfig>, String> {
    #[derive(Default, Deserialize)]
    struct Servers {
        #[serde(default)]
        context_servers: BTreeMap<String, ServerConfig>,
    }
    Ok(config::<Servers>()?.context_servers)
}

fn headers(values: &BTreeMap<String, String>) -> Result<HeaderMap, String> {
    let mut out = HeaderMap::new();
    for (key, value) in values {
        let name = HeaderName::from_bytes(key.as_bytes()).map_err(|_| "Invalid custom header name.")?;
        if matches!(
            name.as_str(),
            "host"
                | "content-length"
                | "transfer-encoding"
                | "connection"
                | "content-type"
                | "accept"
                | "authorization"
                | "proxy-authorization"
                | "cookie"
        ) {
            return Err(format!("Header {name} cannot be overridden; use an API key environment variable for authentication."));
        }
        let mut val = HeaderValue::from_str(value).map_err(|_| "Invalid custom header value.")?;
        val.set_sensitive(true);
        out.insert(name, val);
    }
    Ok(out)
}

fn base_url(raw: &str, ollama: bool) -> Result<String, String> {
    let url = Url::parse(raw).map_err(|_| "Invalid provider api_url.")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Use an HTTP(S) base URL without credentials, query or fragment.".into());
    }
    let base = url.as_str().trim_end_matches('/');
    if ollama {
        return Ok(base
            .strip_suffix("/api/chat")
            .or_else(|| base.strip_suffix("/api"))
            .or_else(|| base.strip_suffix("/v1"))
            .unwrap_or(base)
            .to_owned());
    }
    let base = base.strip_suffix("/chat/completions").unwrap_or(base);
    if url.path().trim_end_matches('/').is_empty() {
        return Ok(format!("{base}/v1"));
    }
    Ok(base.to_owned())
}

fn key_variable(name: &str) -> String {
    let prefix: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' }).collect();
    format!("{prefix}_API_KEY")
}

impl Host {
    fn from_config(name: String, cfg: &Provider, default_url: Option<&str>, key_name: Option<&str>, protocol: Protocol) -> Result<Self, String> {
        if name.trim().is_empty() {
            return Err("A provider name is empty.".into());
        }
        let raw = cfg.api_url.as_deref().or(default_url).ok_or_else(|| format!("Missing api_url for {name}."))?;
        let endpoint = base_url(raw, protocol == Protocol::Ollama)?;
        let derived = key_name.map_or_else(|| key_variable(&name), str::to_owned);
        // A redirected builtin (e.g. OpenAI settings pointed at a different service)
        // must not receive the user's cloud credential unless explicitly selected.
        let key_env = cfg.api_key_env.clone().or_else(|| {
            let url = Url::parse(raw).ok()?;
            let cloud_key = derived == "OPENAI_API_KEY";
            let cloud_origin = url.scheme() == "https" && url.host_str() == Some("api.openai.com") && url.port_or_known_default() == Some(443);
            (!cloud_key || cloud_origin).then_some(derived)
        });
        if key_env.as_ref().is_some_and(|key| key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')) {
            return Err(format!("Invalid API key variable name for {name}."));
        }
        let idle_secs = cfg.low_speed_timeout_in_seconds.unwrap_or(120);
        if cfg.context_window == Some(0) {
            return Err(format!("The context window for {name} must be positive."));
        }
        if !(1..=1800).contains(&idle_secs) {
            return Err(format!("The read timeout for {name} must be 1–1800 seconds."));
        }
        let template = Agent {
            name: String::new(),
            provider: name,
            model: String::new(),
            endpoint,
            supports_tools: true,
            max_tokens: cfg.context_window,
            default_profile: None,
            instructions: String::new(),
            protocol,
            max_output_tokens: None,
            thinking: Thinking::default(),
            parallel: false,
            legacy_limit: false,
            reasoning_history: false,
            completions: true,
            headers: headers(&cfg.custom_headers)?,
            key_env,
            idle_secs,
        };
        Ok(Self { template, discover: cfg.auto_discover.unwrap_or(true) })
    }

    fn model(&self, m: &Model) -> Result<Agent, String> {
        if m.name.trim().is_empty() {
            return Err(format!("A model in {} has no name.", self.template.provider));
        }
        if m.max_output_tokens == Some(0) || m.max_tokens == Some(0) {
            return Err(format!("Token limits for {} must be positive.", m.name));
        }
        let mut a = self.template.clone();
        a.name = m.display_name.as_ref().filter(|n| !n.is_empty()).unwrap_or(&m.name).clone();
        a.model.clone_from(&m.name);
        a.max_tokens = m.max_tokens.or(a.max_tokens);
        a.supports_tools = m.supports_tools.or(m.capabilities.tools).unwrap_or(true);
        a.max_output_tokens = m.max_output_tokens;
        a.thinking = Thinking::configured(a.protocol, m.supports_thinking, m.reasoning_effort.as_ref(), m.reasoning_effort_levels.as_ref())
            .map_err(|e| format!("{} / {}: {e}", a.provider, a.model))?;
        a.parallel = m.capabilities.parallel_tool_calls;
        a.legacy_limit = m.capabilities.max_tokens_parameter;
        a.reasoning_history = m.capabilities.interleaved_reasoning;
        a.completions = m.capabilities.chat_completions != Some(false);
        Ok(a)
    }
}

pub(super) fn catalog() -> Result<Catalog, String> {
    build_catalog(config()?)
}

fn build_catalog(config: Config) -> Result<Catalog, String> {
    let global_profile = profile(config.agent.default_profile)?;
    let mut catalog = Catalog { hosts: Vec::new(), declared: Vec::new(), preferred: config.agent.default_model };
    let models = config.language_models;
    let mut providers = models.openai_compatible.into_iter().map(|(name, p)| (name, p, None, None, Protocol::Completions)).collect::<Vec<_>>();
    for (name, p, url, key, protocol) in [
        ("ollama", models.ollama, "http://localhost:11434", "OLLAMA_API_KEY", Protocol::Ollama),
        ("lmstudio", models.lmstudio, "http://localhost:1234", "LMSTUDIO_API_KEY", Protocol::Completions),
        ("llama.cpp", models.llama_cpp, "http://localhost:8080", "LLAMACPP_API_KEY", Protocol::Completions),
        ("openai", models.openai, "https://api.openai.com/v1", "OPENAI_API_KEY", Protocol::Completions),
    ] {
        if let Some(p) = p {
            providers.push((name.into(), p, Some(url), Some(key), protocol));
        }
    }
    let mut provider_names = BTreeSet::new();
    for (name, p, url, key, protocol) in providers {
        if !provider_names.insert(name.clone()) {
            return Err(format!("Provider name {name} occurs twice."));
        }
        let mut h = Host::from_config(name, &p, url, key, protocol)?;
        h.template.default_profile.clone_from(&global_profile);
        for m in &p.available_models {
            catalog.declared.push(h.model(m)?);
        }
        catalog.hosts.push(h);
    }
    // A default_model is itself an explicit model id; it does not require a
    // running discovery endpoint or a second entry in available_models.
    if let Some(r) = &catalog.preferred
        && !catalog.declared.iter().any(|a| r.provider == a.provider && r.model == a.model)
        && let Some(h) = catalog.hosts.iter().find(|h| r.provider == h.template.provider)
    {
        catalog.declared.push(h.model(&Model { name: r.model.clone(), ..Model::default() })?);
    }
    if let Some(r) = &catalog.preferred {
        for a in catalog.declared.iter_mut().filter(|a| a.provider == r.provider && a.model == r.model) {
            a.thinking.preference(a.protocol, r.enable_thinking, r.effort.as_deref()).map_err(|e| format!("agent.default_model: {e}"))?;
        }
    }
    let base_models = catalog.declared.len();
    for (name, alias) in config.wingman.agents {
        catalog.add_alias(name, alias, &global_profile, base_models)?;
    }
    catalog.sort();
    Ok(catalog)
}

impl Catalog {
    fn add_alias(&mut self, name: String, alias: Alias, global_profile: &Option<String>, base_models: usize) -> Result<(), String> {
        if name.trim().is_empty() {
            return Err("An agent name is empty.".into());
        }
        if alias.instructions.len() > 8192 {
            return Err(format!("Instructions for {name} exceed 8 KiB."));
        }
        let r = alias.target;
        // An alias inherits model settings, never a preceding alias's instructions or thinking preference.
        let mut a = if let Some(a) = self.declared[..base_models].iter().find(|a| r.provider == a.provider && r.model == a.model) {
            a.clone()
        } else {
            self.hosts
                .iter()
                .find(|h| r.provider == h.template.provider)
                .ok_or_else(|| format!("Unknown provider for agent {name}."))?
                .model(&Model { name: r.model, ..Model::default() })?
        };
        a.name = name;
        a.thinking.preference(a.protocol, r.enable_thinking, r.effort.as_deref()).map_err(|e| format!("wingman.agents / {}: {e}", a.name))?;
        a.instructions = alias.instructions;
        a.default_profile = profile(alias.default_profile)?.or_else(|| global_profile.clone());
        self.declared.push(a);
        Ok(())
    }
    pub(super) fn sort(&mut self) {
        self.declared.sort_by_key(|a| !self.preferred.as_ref().is_some_and(|m| m.provider == a.provider && m.model == a.model));
    }
}
fn profile(name: Option<String>) -> Result<Option<String>, String> {
    if name.as_deref().is_some_and(|s| !matches!(s, "ask" | "write" | "write_mcp" | "minimal")) {
        return Err("Wingman default_profile must be ask, write, write_mcp or minimal.".into());
    }
    Ok(name)
}

pub(in super::super) fn load() -> Result<Vec<Agent>, String> {
    Ok(catalog()?.declared)
}

#[cfg(test)]
mod tests {
    use super::build_catalog;
    use serde_json::json;

    #[test]
    fn model_ref_and_alias_preferences_reach_separate_requests_for_the_same_model() {
        let catalog = build_catalog(
            serde_json::from_value(json!({
                "language_models": {"openai_compatible": {"desk": {
                    "api_url":"http://localhost:8000/v1",
                    "available_models":[{"name":"demo","reasoning_effort_levels":"none,low,high"}]
                }}},
                "agent":{"default_model":{"provider":"desk","model":"demo","effort":"low","enable_thinking":true}},
                "wingman":{"agents":{
                    "Deliberate":{"provider":"desk","model":"demo","effort":"high"},
                    "Fast":{"provider":"desk","model":"demo","enable_thinking":false}
                }}
            }))
            .unwrap(),
        )
        .unwrap();
        let effort = |name: &str| {
            let a = catalog.declared.iter().find(|a| a.name == name).unwrap();
            super::super::transport::payload(a, Vec::new(), Vec::new()).unwrap()["reasoning_effort"].clone()
        };
        assert_eq!(effort("demo"), "low");
        assert_eq!(effort("Deliberate"), "high");
        assert_eq!(effort("Fast"), "none");
    }
}
