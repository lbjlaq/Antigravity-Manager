use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf, time::Duration};
use tokio::process::Command;
use yaml_rt::{JsonPointer, NodeId, SemanticKind, YamlDoc, YamlFragment};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

const HERMES_DIR: &str = ".hermes";
const HERMES_CONFIG_FILE: &str = "config.yaml";
const BACKUP_SUFFIX: &str = ".antigravity-manager.bak";
const PROVIDER_ID: &str = "antigravity-manager";
const PROVIDER_DISPLAY_NAME: &str = "Antigravity Manager";
const PROVIDER_REF: &str = "custom:antigravity-manager";
const EMPTY_CONFIG: &str = "{}\n";

static HERMES_CONFIG_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn acquire_hermes_config_lock() -> std::sync::MutexGuard<'static, ()> {
    HERMES_CONFIG_MUTEX.lock().unwrap_or_else(|poisoned| {
        tracing::warn!("HERMES_CONFIG_MUTEX was poisoned, recovering lock");
        poisoned.into_inner()
    })
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HermesStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub is_synced: bool,
    pub has_backup: bool,
    pub current_base_url: Option<String>,
    pub files: Vec<String>,
    pub discover_models: bool,
    pub configured_models: Vec<String>,
    pub is_active: bool,
    pub default_model: Option<String>,
}

fn get_hermes_dir() -> Option<PathBuf> {
    env::var_os("HERMES_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(HERMES_DIR)))
}

fn get_config_path() -> Option<PathBuf> {
    get_hermes_dir().map(|dir| dir.join(HERMES_CONFIG_FILE))
}

fn get_backup_path() -> Option<PathBuf> {
    get_config_path().map(|path| {
        path.with_file_name(format!(
            "{}{}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            BACKUP_SUFFIX
        ))
    })
}

fn normalize_base_url(input: &str) -> String {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1")
    }
}

fn find_in_path(executable: &str) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        for dir in env::var("PATH").ok()?.split(';') {
            for ext in ["exe", "cmd", "bat"] {
                let path = PathBuf::from(dir).join(format!("{executable}.{ext}"));
                if path.exists() {
                    return Some(path);
                }
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        for dir in env::var("PATH").ok()?.split(':') {
            let path = PathBuf::from(dir).join(executable);
            if path.exists() {
                return Some(path);
            }
        }
    }
    None
}

fn resolve_hermes_path() -> Option<PathBuf> {
    if let Some(path) = find_in_path("hermes") {
        return Some(path);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home = dirs::home_dir()?;
        for path in [
            home.join(".hermes/bin/hermes"),
            home.join(".local/bin/hermes"),
            PathBuf::from("/opt/homebrew/bin/hermes"),
            PathBuf::from("/usr/local/bin/hermes"),
            PathBuf::from("/usr/bin/hermes"),
        ] {
            if path.exists() {
                return Some(path);
            }
        }
    }
    None
}

fn extract_version(raw: &str) -> String {
    let trimmed = raw.trim();
    for part in trimmed.split_whitespace() {
        let candidate = part.rsplit('/').next().unwrap_or(part);
        let clean = candidate
            .strip_prefix('v')
            .or_else(|| candidate.strip_prefix('V'))
            .unwrap_or(candidate);
        if is_valid_version(clean) {
            return clean.to_string();
        }
    }
    let fallback: String = trimmed
        .chars()
        .skip_while(|character| !character.is_ascii_digit())
        .take_while(|character| character.is_ascii_digit() || *character == '.')
        .collect();
    if fallback.contains('.') {
        fallback
    } else {
        "unknown".to_string()
    }
}

fn is_valid_version(value: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_digit())
        && value.contains('.')
        && value
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.')
}

async fn run_version_command(mut command: Command, timeout: Duration) -> Option<String> {
    command.kill_on_drop(true);
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);
    let output = tokio::time::timeout(timeout, command.output()).await;

    match output {
        Ok(Ok(output)) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            Some(extract_version(if stdout.trim().is_empty() {
                &stderr
            } else {
                &stdout
            }))
        }
        _ => None,
    }
}

async fn run_hermes_version(path: &PathBuf) -> Option<String> {
    let mut command = Command::new(path);
    command.arg("--version");
    run_version_command(command, VERSION_PROBE_TIMEOUT).await
}

pub async fn check_hermes_installed() -> (bool, Option<String>) {
    match resolve_hermes_path() {
        Some(path) => (true, run_hermes_version(&path).await),
        None => (false, None),
    }
}

fn parse_pointer(path: &str) -> Result<JsonPointer, String> {
    JsonPointer::parse(path).map_err(|error| format!("Invalid YAML path {path:?}: {error}"))
}

fn parse_doc(source: &str) -> Result<YamlDoc, String> {
    let source = if source.trim().is_empty() {
        EMPTY_CONFIG
    } else {
        source
    };
    YamlDoc::parse(source).map_err(|error| format!("Hermes config.yaml is not valid YAML: {error}"))
}

fn render_doc(doc: &YamlDoc) -> String {
    doc.as_source().to_string()
}

fn read_hermes_source(path: &PathBuf) -> Result<String, String> {
    match fs::read_to_string(path) {
        Ok(source) if source.trim().is_empty() => Ok(EMPTY_CONFIG.to_string()),
        Ok(source) => {
            parse_doc(&source)?;
            Ok(source)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(EMPTY_CONFIG.to_string()),
        Err(error) => Err(format!("Failed to read Hermes config: {error}")),
    }
}

fn atomically_write_source(path: &PathBuf, source: &str) -> Result<(), String> {
    parse_doc(source)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create directory: {error}"))?;
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(HERMES_CONFIG_FILE);
    let temp = path.with_file_name(format!("{file_name}.tmp.{}", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temp)
        .map_err(|error| format!("Failed to create temp file: {error}"))?;
    let result = (|| -> std::io::Result<()> {
        use std::io::Write;
        #[cfg(unix)]
        if let Ok(metadata) = fs::metadata(path) {
            file.set_permissions(metadata.permissions())?;
        }
        file.write_all(source.as_bytes())?;
        file.sync_all()
    })();
    drop(file);
    if let Err(error) = result {
        let _ = fs::remove_file(&temp);
        return Err(format!("Failed to write temp file: {error}"));
    }
    fs::rename(&temp, path).map_err(|error| {
        let _ = fs::remove_file(&temp);
        format!("Failed to rename config file: {error}")
    })
}

fn create_backup(path: &PathBuf) -> Result<(), String> {
    let backup = path.with_file_name(format!(
        "{}{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        BACKUP_SUFFIX
    ));
    if !backup.exists() {
        // Preserve an empty baseline too, so the first sync can be undone.
        atomically_write_source(&backup, &read_hermes_source(path)?)?;
    }
    Ok(())
}

fn resolve_optional(doc: &YamlDoc, path: &str) -> Option<NodeId> {
    doc.resolve_pointer(0, &parse_pointer(path).ok()?).ok()
}

fn scalar_at(doc: &YamlDoc, path: &str) -> Option<String> {
    doc.scalar_value(resolve_optional(doc, path)?)
        .ok()
        .map(|value| value.into_owned())
}

fn bool_at(doc: &YamlDoc, path: &str) -> Option<bool> {
    match scalar_at(doc, path)?.to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn is_mapping_at(doc: &YamlDoc, path: &str) -> bool {
    resolve_optional(doc, path)
        .is_some_and(|node| matches!(doc.semantic_kind(node), Some(SemanticKind::Mapping { .. })))
}

fn mapping_len_at(doc: &YamlDoc, path: &str) -> Option<usize> {
    let node = resolve_optional(doc, path)?;
    matches!(doc.semantic_kind(node), Some(SemanticKind::Mapping { .. }))
        .then(|| doc.mapping_entries(node).count())
}

fn string_list_at(doc: &YamlDoc, path: &str) -> Vec<String> {
    let Some(node) = resolve_optional(doc, path) else {
        return Vec::new();
    };
    match doc.semantic_kind(node) {
        Some(SemanticKind::Sequence { .. }) => doc
            .sequence_items(node)
            .filter_map(|item| doc.scalar_value(item).ok().map(|value| value.into_owned()))
            .collect(),
        Some(SemanticKind::Mapping { .. }) => doc
            .mapping_entries(node)
            .filter_map(|(key, _)| doc.scalar_value(key).ok().map(|value| value.into_owned()))
            .collect(),
        _ => Vec::new(),
    }
}

fn fragment(source: &str) -> Result<YamlFragment, String> {
    YamlFragment::parse(source).map_err(|error| format!("Failed to build YAML fragment: {error}"))
}

fn string_fragment(value: &str) -> Result<YamlFragment, String> {
    fragment(
        &serde_json::to_string(value)
            .map_err(|error| format!("Failed to encode YAML string: {error}"))?,
    )
}

fn bool_fragment(value: bool) -> Result<YamlFragment, String> {
    fragment(if value { "true" } else { "false" })
}

fn preferred_line_ending(source: &str) -> &'static str {
    if source.contains("\r\n") {
        "\r\n"
    } else if source.contains('\r') {
        "\r"
    } else {
        "\n"
    }
}

fn sequence_fragment(values: &[String], line_ending: &str) -> Result<YamlFragment, String> {
    if values.is_empty() {
        return fragment("[]");
    }
    let mut source = String::new();
    for value in values {
        source.push_str("- ");
        source.push_str(
            &serde_json::to_string(value)
                .map_err(|error| format!("Failed to encode model id: {error}"))?,
        );
        source.push_str(line_ending);
    }
    fragment(&source)
}

fn provider_fragment(
    base_url: &str,
    api_key: &str,
    discover_models: bool,
    models: &[String],
    line_ending: &str,
) -> Result<YamlFragment, String> {
    let encode = |value: &str| {
        serde_json::to_string(value).map_err(|error| format!("Failed to encode provider: {error}"))
    };
    let mut source = [
        format!("name: {}", encode(PROVIDER_DISPLAY_NAME)?),
        format!("api: {}", encode(base_url)?),
        format!("api_key: {}", encode(api_key)?),
        "transport: chat_completions".to_string(),
        format!("discover_models: {discover_models}"),
    ]
    .join(line_ending);
    source.push_str(line_ending);
    if !discover_models {
        source.push_str("models:");
        source.push_str(line_ending);
        for model in models {
            source.push_str("  - ");
            source.push_str(&encode(model)?);
            source.push_str(line_ending);
        }
    }
    fragment(&source)
}

fn commit(doc: &mut YamlDoc) -> Result<(), String> {
    doc.commit_edits()
        .map_err(|error| format!("Failed to apply Hermes YAML edit: {error}"))
}

fn upsert(doc: &mut YamlDoc, path: &str, value: &YamlFragment) -> Result<(), String> {
    let pointer = parse_pointer(path)?;
    if doc.resolve_pointer(0, &pointer).is_ok() {
        doc.replace_at(0, &pointer, value)
            .map_err(|error| format!("Failed to replace YAML path {path}: {error}"))?;
    } else {
        doc.add_at(0, &pointer, value)
            .map_err(|error| format!("Failed to add YAML path {path}: {error}"))?;
    }
    commit(doc)
}

fn remove(doc: &mut YamlDoc, path: &str) -> Result<bool, String> {
    let pointer = parse_pointer(path)?;
    if doc.resolve_pointer(0, &pointer).is_err() {
        return Ok(false);
    }
    doc.remove_at(0, &pointer)
        .map_err(|error| format!("Failed to remove YAML path {path}: {error}"))?;
    commit(doc)?;
    Ok(true)
}

fn ensure_root_mapping(doc: &mut YamlDoc) -> Result<(), String> {
    let root = doc
        .document_root(0)
        .map_err(|error| format!("Failed to inspect Hermes YAML root: {error}"))?;
    if root
        .is_some_and(|node| matches!(doc.semantic_kind(node), Some(SemanticKind::Mapping { .. })))
    {
        Ok(())
    } else {
        upsert(doc, "", &fragment("{}")?)
    }
}

fn ensure_mapping(doc: &mut YamlDoc, path: &str) -> Result<(), String> {
    if is_mapping_at(doc, path) {
        Ok(())
    } else {
        upsert(doc, path, &fragment("{}")?)
    }
}

fn sync_sequence(
    doc: &mut YamlDoc,
    path: &str,
    values: &[String],
    line_ending: &str,
) -> Result<(), String> {
    let Some(node) = resolve_optional(doc, path) else {
        return upsert(doc, path, &sequence_fragment(values, line_ending)?);
    };
    if !matches!(doc.semantic_kind(node), Some(SemanticKind::Sequence { .. })) {
        return upsert(doc, path, &sequence_fragment(values, line_ending)?);
    }
    let current_len = doc.sequence_items(node).count();
    for (index, value) in values.iter().take(current_len).enumerate() {
        upsert(doc, &format!("{path}/{index}"), &string_fragment(value)?)?;
    }
    for index in (values.len()..current_len).rev() {
        remove(doc, &format!("{path}/{index}"))?;
    }
    for value in values.iter().skip(current_len) {
        upsert(doc, &format!("{path}/-"), &string_fragment(value)?)?;
    }
    Ok(())
}

fn extract_fragment(doc: &YamlDoc, path: &str) -> Result<Option<YamlFragment>, String> {
    let Some(node) = resolve_optional(doc, path) else {
        return Ok(None);
    };
    fragment(
        &doc.extract_node(node)
            .map_err(|error| format!("Failed to extract backup path {path}: {error}"))?,
    )
    .map(Some)
}

fn escape_pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

fn mapping_keys(doc: &YamlDoc, path: &str) -> Vec<String> {
    let Some(node) = resolve_optional(doc, path) else {
        return Vec::new();
    };
    doc.mapping_entries(node)
        .filter_map(|(key, _)| doc.scalar_value(key).ok().map(|value| value.into_owned()))
        .collect()
}

fn restore_mapping(current: &mut YamlDoc, backup: &YamlDoc, path: &str) -> Result<(), String> {
    ensure_mapping(current, path)?;
    let backup_keys = mapping_keys(backup, path);
    for key in mapping_keys(current, path) {
        if !backup_keys.iter().any(|backup_key| backup_key == &key) {
            remove(current, &format!("{path}/{}", escape_pointer_token(&key)))?;
        }
    }
    for key in backup_keys {
        let child = format!("{path}/{}", escape_pointer_token(&key));
        if let Some(value) = extract_fragment(backup, &child)? {
            upsert(current, &child, &value)?;
        }
    }
    Ok(())
}

fn is_managed_provider(provider: &str) -> bool {
    matches!(provider, PROVIDER_REF | PROVIDER_ID)
}

fn model_has_only_managed_fields(doc: &YamlDoc) -> bool {
    let Some(model) = resolve_optional(doc, "/model") else {
        return false;
    };
    matches!(doc.semantic_kind(model), Some(SemanticKind::Mapping { .. }))
        && doc.mapping_entries(model).all(|(key, _)| {
            doc.scalar_value(key)
                .is_ok_and(|value| matches!(value.as_ref(), "provider" | "default"))
        })
}

fn deactivate(doc: &mut YamlDoc, backup: Option<&YamlDoc>) -> Result<(), String> {
    let Some(current) = scalar_at(doc, "/model/provider") else {
        return Ok(());
    };
    if !is_managed_provider(&current) {
        return Ok(());
    }
    let safe_backup = backup.filter(|value| {
        !scalar_at(value, "/model/provider").is_some_and(|provider| is_managed_provider(&provider))
    });
    if let Some(backup) = safe_backup {
        if let Some(provider) = extract_fragment(backup, "/model/provider")? {
            upsert(doc, "/model/provider", &provider)?;
        } else {
            remove(doc, "/model/provider")?;
        }
        if let Some(default_model) = extract_fragment(backup, "/model/default")? {
            upsert(doc, "/model/default", &default_model)?;
        } else {
            remove(doc, "/model/default")?;
        }
    } else if model_has_only_managed_fields(doc) {
        remove(doc, "/model")?;
    } else {
        remove(doc, "/model/provider")?;
        remove(doc, "/model/default")?;
    }
    if mapping_len_at(doc, "/model") == Some(0) {
        remove(doc, "/model")?;
    }
    Ok(())
}

fn apply_sync_losslessly(
    source: &str,
    base_url: &str,
    api_key: &str,
    discover_models: bool,
    models: &[String],
    activate: bool,
    default_model: Option<&str>,
    backup: Option<&str>,
) -> Result<String, String> {
    let line_ending = preferred_line_ending(source);
    let mut doc = parse_doc(source)?;
    let backup = backup.map(parse_doc).transpose()?;
    ensure_root_mapping(&mut doc)?;
    ensure_mapping(&mut doc, "/providers")?;
    let provider = format!("/providers/{PROVIDER_ID}");
    if !is_mapping_at(&doc, &provider) {
        upsert(
            &mut doc,
            &provider,
            &provider_fragment(base_url, api_key, discover_models, models, line_ending)?,
        )?;
    } else {
        for key in [
            "key_cmd",
            "key_env",
            "api_key_env",
            "apiKey",
            "keyEnv",
            "apiKeyEnv",
            "base_url",
            "url",
            "api_mode",
        ] {
            remove(&mut doc, &format!("{provider}/{key}"))?;
        }
        for (key, value) in [
            ("name", PROVIDER_DISPLAY_NAME),
            ("api", base_url),
            ("api_key", api_key),
            ("transport", "chat_completions"),
        ] {
            upsert(
                &mut doc,
                &format!("{provider}/{key}"),
                &string_fragment(value)?,
            )?;
        }
        upsert(
            &mut doc,
            &format!("{provider}/discover_models"),
            &bool_fragment(discover_models)?,
        )?;
        let model_path = format!("{provider}/models");
        if discover_models {
            remove(&mut doc, &model_path)?;
        } else {
            sync_sequence(&mut doc, &model_path, models, line_ending)?;
        }
    }

    if activate {
        ensure_mapping(&mut doc, "/model")?;
        upsert(&mut doc, "/model/provider", &string_fragment(PROVIDER_REF)?)?;
        if let Some(model) = default_model.filter(|model| !model.trim().is_empty()) {
            upsert(&mut doc, "/model/default", &string_fragment(model)?)?;
        }
    } else {
        deactivate(&mut doc, backup.as_ref())?;
    }
    Ok(render_doc(&doc))
}

fn apply_clear_losslessly(source: &str, backup: Option<&str>) -> Result<(String, bool), String> {
    let mut doc = parse_doc(source)?;
    let backup_doc = backup.map(parse_doc).transpose()?;
    let mut changed = false;

    if scalar_at(&doc, "/model/provider").is_some_and(|provider| is_managed_provider(&provider)) {
        deactivate(&mut doc, backup_doc.as_ref())?;
        changed = true;
    }

    let provider = format!("/providers/{PROVIDER_ID}");
    if resolve_optional(&doc, &provider).is_some() {
        if mapping_len_at(&doc, "/providers") == Some(1) {
            remove(&mut doc, "/providers")?;
        } else {
            remove(&mut doc, &provider)?;
        }
        changed = true;
    }
    Ok((render_doc(&doc), changed))
}

fn apply_restore_losslessly(current: &str, backup: &str) -> Result<String, String> {
    let current_source = current;
    let mut current = parse_doc(current)?;
    let backup = parse_doc(backup)?;
    ensure_root_mapping(&mut current)?;
    let provider = format!("/providers/{PROVIDER_ID}");
    if is_mapping_at(&backup, &provider) {
        ensure_mapping(&mut current, "/providers")?;
        restore_mapping(&mut current, &backup, &provider)?;
    } else if resolve_optional(&current, &provider).is_some() {
        if mapping_len_at(&current, "/providers") == Some(1) {
            remove(&mut current, "/providers")?;
        } else {
            remove(&mut current, &provider)?;
        }
    }

    if scalar_at(&current, "/model/provider").is_some_and(|provider| is_managed_provider(&provider))
    {
        if let Some(value) = extract_fragment(&backup, "/model/provider")? {
            upsert(&mut current, "/model/provider", &value)?;
        } else {
            remove(&mut current, "/model/provider")?;
        }
        if resolve_optional(&current, "/model").is_some() {
            if let Some(value) = extract_fragment(&backup, "/model/default")? {
                upsert(&mut current, "/model/default", &value)?;
            } else {
                remove(&mut current, "/model/default")?;
            }
        }
        if mapping_len_at(&current, "/model") == Some(0) {
            remove(&mut current, "/model")?;
        }
    }
    Ok(render_doc(&current))
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key == "api_key"
        || key == "apikey"
        || key.ends_with("_api_key")
        || key == "password"
        || key.ends_with("_password")
        || key == "secret"
        || key.ends_with("_secret")
        || key == "token"
        || key.ends_with("_token")
        || key == "credential"
        || key.ends_with("_credential")
        || key == "private_key"
        || key.ends_with("_private_key")
}

fn collect_sensitive_paths(doc: &YamlDoc, node: NodeId, path: &str, output: &mut Vec<String>) {
    match doc.semantic_kind(node) {
        Some(SemanticKind::Mapping { .. }) => {
            for (key_node, value_node) in doc.mapping_entries(node).collect::<Vec<_>>() {
                let Ok(key) = doc.scalar_value(key_node) else {
                    continue;
                };
                let escaped = escape_pointer_token(&key);
                let child = format!("{path}/{escaped}");
                if is_sensitive_key(&key) {
                    output.push(child);
                } else {
                    collect_sensitive_paths(doc, value_node, &child, output);
                }
            }
        }
        Some(SemanticKind::Sequence { .. }) => {
            for (index, item) in doc
                .sequence_items(node)
                .collect::<Vec<_>>()
                .into_iter()
                .enumerate()
            {
                collect_sensitive_paths(doc, item, &format!("{path}/{index}"), output);
            }
        }
        _ => {}
    }
}

fn redact_sensitive_source(source: &str) -> Result<String, String> {
    let mut doc = parse_doc(source)?;
    let root = doc
        .document_root(0)
        .map_err(|error| format!("Failed to inspect Hermes config: {error}"))?;
    let Some(root) = root else {
        return Ok(render_doc(&doc));
    };
    let mut paths = Vec::new();
    collect_sensitive_paths(&doc, root, "", &mut paths);
    let redacted = string_fragment("[REDACTED]")?;
    for path in paths {
        upsert(&mut doc, &path, &redacted)?;
    }
    Ok(render_doc(&doc))
}

fn read_provider_entry(doc: &YamlDoc) -> Option<(Option<String>, Option<String>, Option<String>)> {
    let provider = format!("/providers/{PROVIDER_ID}");
    is_mapping_at(doc, &provider).then_some((
        scalar_at(doc, &format!("{provider}/api"))
            .or_else(|| scalar_at(doc, &format!("{provider}/base_url")))
            .or_else(|| scalar_at(doc, &format!("{provider}/url"))),
        scalar_at(doc, &format!("{provider}/api_key")),
        scalar_at(doc, &format!("{provider}/transport"))
            .or_else(|| scalar_at(doc, &format!("{provider}/api_mode"))),
    ))
}

#[derive(Default)]
struct HermesConfigState {
    is_synced: bool,
    has_backup: bool,
    current_base_url: Option<String>,
    discover_models: bool,
    configured_models: Vec<String>,
    is_active: bool,
    default_model: Option<String>,
}

fn read_config_state(proxy_url: Option<String>) -> HermesConfigState {
    let mut state = HermesConfigState {
        has_backup: get_backup_path().is_some_and(|path| path.exists()),
        discover_models: true,
        ..HermesConfigState::default()
    };
    let Some(path) = get_config_path() else {
        return state;
    };
    let Ok(source) = read_hermes_source(&path) else {
        return state;
    };
    let Ok(doc) = parse_doc(&source) else {
        return state;
    };
    let provider = format!("/providers/{PROVIDER_ID}");
    if is_mapping_at(&doc, &provider) {
        state.discover_models =
            bool_at(&doc, &format!("{provider}/discover_models")).unwrap_or(true);
        state.configured_models = string_list_at(&doc, &format!("{provider}/models"));
    }
    let active = scalar_at(&doc, "/model/provider").unwrap_or_default();
    state.is_active = is_managed_provider(&active);
    if state.is_active {
        state.default_model =
            scalar_at(&doc, "/model/default").or_else(|| scalar_at(&doc, "/model/name"));
    }
    let Some((base_url, api_key, _)) = read_provider_entry(&doc) else {
        return state;
    };
    state.current_base_url = base_url.clone();
    let (Some(url), Some(key)) = (base_url, api_key) else {
        return state;
    };
    if url.trim().is_empty() || key.trim().is_empty() {
        return state;
    }
    state.is_synced = proxy_url
        .filter(|value| !value.trim().is_empty())
        .map(|expected| normalize_base_url(&url) == normalize_base_url(&expected))
        .unwrap_or(true);
    state
}

pub fn sync_hermes_provider(
    proxy_url: String,
    api_key: String,
    discover_models: bool,
    models: Vec<String>,
    activate: bool,
    default_model: Option<String>,
) -> Result<(), String> {
    let _lock = acquire_hermes_config_lock();
    let normalized_url = normalize_base_url(&proxy_url);
    if normalized_url.trim().is_empty() || normalized_url == "/v1" || api_key.trim().is_empty() {
        return Err("Hermes base URL and API key are required".to_string());
    }
    let models: Vec<String> = models
        .into_iter()
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty())
        .collect();
    if !discover_models && models.is_empty() {
        return Err("Select at least one model or enable automatic model discovery".to_string());
    }
    let selected_default = default_model
        .as_deref()
        .filter(|model| !model.trim().is_empty())
        .or_else(|| models.first().map(String::as_str));
    if activate && selected_default.is_none() {
        return Err("Select a default model before activating Antigravity Manager".to_string());
    }
    if activate
        && !discover_models
        && selected_default.is_some_and(|default| !models.iter().any(|model| model == default))
    {
        return Err("The default model must be included in the selected Hermes models".to_string());
    }

    let path = get_config_path().ok_or("Failed to get Hermes config directory")?;
    let source = read_hermes_source(&path)?;
    create_backup(&path)?;
    let backup = get_backup_path()
        .filter(|path| path.exists())
        .map(|path| read_hermes_source(&path))
        .transpose()?;
    let updated = apply_sync_losslessly(
        &source,
        &normalized_url,
        api_key.trim(),
        discover_models,
        &models,
        activate,
        selected_default,
        backup.as_deref(),
    )?;
    atomically_write_source(&path, &updated)
}

pub fn restore_hermes_config() -> Result<(), String> {
    let _lock = acquire_hermes_config_lock();
    let path = get_config_path().ok_or("Failed to get Hermes config directory")?;
    let backup_path = get_backup_path().ok_or("Failed to get Hermes config directory")?;
    if !backup_path.exists() {
        return Err("No backup file found".to_string());
    }
    let restored = apply_restore_losslessly(
        &read_hermes_source(&path)?,
        &read_hermes_source(&backup_path)?,
    )?;
    atomically_write_source(&path, &restored)?;
    fs::remove_file(backup_path).map_err(|error| format!("Failed to remove backup: {error}"))
}

pub fn clear_hermes_config() -> Result<(), String> {
    let _lock = acquire_hermes_config_lock();
    let path = get_config_path().ok_or("Failed to get Hermes config directory")?;
    if !path.exists() {
        return Ok(());
    }
    let source = read_hermes_source(&path)?;
    let backup = get_backup_path()
        .filter(|path| path.exists())
        .map(|path| read_hermes_source(&path))
        .transpose()?;
    let (updated, changed) = apply_clear_losslessly(&source, backup.as_deref())?;
    if !changed {
        return Ok(());
    }
    create_backup(&path)?;
    atomically_write_source(&path, &updated)
}

pub fn read_hermes_config_content() -> Result<String, String> {
    let _lock = acquire_hermes_config_lock();
    let path = get_config_path().ok_or("Failed to get Hermes config directory")?;
    if !path.exists() {
        return Err(format!("Config file does not exist: {path:?}"));
    }
    redact_sensitive_source(&read_hermes_source(&path)?)
}

#[tauri::command]
pub async fn get_hermes_sync_status(proxy_url: Option<String>) -> Result<HermesStatus, String> {
    // CLI startup can be slow or hang; never block configuration operations on it.
    let (installed, version) = check_hermes_installed().await;
    tokio::task::spawn_blocking(move || {
        let _lock = acquire_hermes_config_lock();
        let state = read_config_state(proxy_url);
        Ok(HermesStatus {
            installed,
            version,
            is_synced: state.is_synced,
            has_backup: state.has_backup,
            current_base_url: state.current_base_url,
            files: vec![HERMES_CONFIG_FILE.to_string()],
            discover_models: state.discover_models,
            configured_models: state.configured_models,
            is_active: state.is_active,
            default_model: state.default_model,
        })
    })
    .await
    .unwrap_or_else(|_| Err("Failed to execute check".to_string()))
}

#[tauri::command]
pub async fn execute_hermes_sync(
    proxy_url: String,
    api_key: String,
    discover_models: bool,
    models: Vec<String>,
    activate: bool,
    default_model: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        sync_hermes_provider(
            proxy_url,
            api_key,
            discover_models,
            models,
            activate,
            default_model,
        )
    })
    .await
    .unwrap_or_else(|_| Err("Failed to execute sync".to_string()))
}

#[tauri::command]
pub async fn execute_hermes_restore() -> Result<(), String> {
    tokio::task::spawn_blocking(restore_hermes_config)
        .await
        .unwrap_or_else(|_| Err("Failed to execute restore".to_string()))
}

#[tauri::command]
pub async fn execute_hermes_clear() -> Result<(), String> {
    tokio::task::spawn_blocking(clear_hermes_config)
        .await
        .unwrap_or_else(|_| Err("Failed to execute clear".to_string()))
}

#[tauri::command]
pub async fn get_hermes_config_content() -> Result<String, String> {
    tokio::task::spawn_blocking(read_hermes_config_content)
        .await
        .unwrap_or_else(|_| Err("Failed to read config".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version_probe_test_command(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "proxy::hermes_sync::tests::version_probe_subprocess_helper",
                "--nocapture",
            ])
            .env("ANTIGRAVITY_HERMES_VERSION_PROBE_TEST", mode);
        command
    }

    #[test]
    fn version_probe_subprocess_helper() {
        match std::env::var("ANTIGRAVITY_HERMES_VERSION_PROBE_TEST").as_deref() {
            Ok("success") => println!("Hermes Agent v0.21.3 (2026.9.14)"),
            Ok("timeout") => std::thread::sleep(Duration::from_secs(60)),
            _ => {}
        }
    }

    #[tokio::test]
    async fn version_probe_extracts_version_from_successful_subprocess() {
        assert_eq!(
            run_version_command(
                version_probe_test_command("success"),
                Duration::from_secs(10)
            )
            .await
            .as_deref(),
            Some("0.21.3")
        );
    }

    #[tokio::test]
    async fn version_probe_times_out_hung_subprocess() {
        assert!(run_version_command(
            version_probe_test_command("timeout"),
            Duration::from_millis(100)
        )
        .await
        .is_none());
    }

    fn doc(source: &str) -> YamlDoc {
        parse_doc(source).expect("valid yaml")
    }

    #[test]
    fn extract_version_accepts_v_prefixed_hermes_output() {
        assert_eq!(
            extract_version("Hermes Agent v0.21.3 (2026.9.14)"),
            "0.21.3"
        );
        assert_eq!(extract_version("hermes/V1.2.0"), "1.2.0");
    }

    #[test]
    fn sync_creates_provider_with_selected_models() {
        let models = vec!["gemini-2.5-pro".into(), "claude-sonnet-4-6".into()];
        let updated = apply_sync_losslessly(
            EMPTY_CONFIG,
            "http://127.0.0.1:8045/v1",
            "sk-test",
            false,
            &models,
            false,
            None,
            None,
        )
        .unwrap();
        let parsed = doc(&updated);
        assert_eq!(
            scalar_at(&parsed, "/providers/antigravity-manager/api").as_deref(),
            Some("http://127.0.0.1:8045/v1")
        );
        assert_eq!(
            string_list_at(&parsed, "/providers/antigravity-manager/models"),
            models
        );
    }

    #[test]
    fn sync_preserves_comments_formatting_and_unrelated_providers() {
        let source = "# user header\nproviders: # provider registry\n  other:\n    api: 'https://example.test/v1' # untouched\n  antigravity-manager:\n    name: Old Name # managed comment\n    api: http://old.test/v1 # endpoint comment\n    api_key: old-key # credential comment\n    transport: chat_completions\n    discover_models: false\n    models:\n      - old-model # model comment\ndisplay: {theme: custom} # flow stays flow\n";
        let updated = apply_sync_losslessly(
            source,
            "http://127.0.0.1:8045/v1",
            "sk-new",
            false,
            &["new-model".into()],
            false,
            None,
            None,
        )
        .unwrap();
        for expected in [
            "# user header\nproviders: # provider registry",
            "api: 'https://example.test/v1' # untouched",
            "name: Antigravity Manager # managed comment",
            "api: http://127.0.0.1:8045/v1 # endpoint comment",
            "api_key: sk-new # credential comment",
            "- new-model # model comment",
            "display: {theme: custom} # flow stays flow",
        ] {
            assert!(
                updated.contains(expected),
                "missing {expected:?}\n{updated}"
            );
        }
    }

    #[test]
    fn identical_sync_is_byte_for_byte_unchanged() {
        let source = "# exact bytes\nproviders:\n  antigravity-manager:\n    name: 'Antigravity Manager'\n    api: http://127.0.0.1:8045/v1\n    api_key: sk-test\n    transport: chat_completions\n    discover_models: true\nother: 0x10 # spelling\n";
        assert_eq!(
            apply_sync_losslessly(
                source,
                "http://127.0.0.1:8045/v1",
                "sk-test",
                true,
                &[],
                false,
                None,
                None
            )
            .unwrap(),
            source
        );
    }

    #[test]
    fn sync_accepts_and_preserves_hermes_indentless_toolset_sequences() {
        let toolsets = "platform_toolsets:\n  cli:\n  - hermes-cli\n  telegram:\n  - hermes-telegram\n  discord:\n  - hermes-discord\n";
        let source =
            format!("{toolsets}providers:\n  existing:\n    api: https://example.test/v1\n");
        let updated = apply_sync_losslessly(
            &source,
            "http://127.0.0.1:8045/v1",
            "sk-test",
            true,
            &[],
            false,
            None,
            None,
        )
        .unwrap();

        assert!(updated.starts_with(toolsets));
        assert!(updated.contains("  antigravity-manager:\n"));
        assert!(parse_doc(&updated).is_ok());
    }

    #[test]
    fn activation_preserves_model_comments_and_unknown_settings() {
        let source = "model:\n  provider: openrouter # selected provider\n  default: old-model # selected model\n  fallback: keep-me\n";
        let updated = apply_sync_losslessly(
            source,
            "http://127.0.0.1:8045/v1",
            "sk-test",
            true,
            &[],
            true,
            Some("gemini-2.5-pro"),
            None,
        )
        .unwrap();
        let parsed = doc(&updated);
        assert_eq!(
            scalar_at(&parsed, "/model/provider").as_deref(),
            Some(PROVIDER_REF)
        );
        assert_eq!(
            scalar_at(&parsed, "/model/fallback").as_deref(),
            Some("keep-me")
        );
        assert!(updated.contains("provider: custom:antigravity-manager # selected provider"));
        assert!(updated.contains("default: gemini-2.5-pro # selected model"));
    }

    #[test]
    fn deactivation_restores_previous_provider() {
        let backup = "model:\n  provider: openai-codex # original provider\n  default: gpt-5-codex # original model\n";
        let current = "model:\n  provider: custom:antigravity-manager # original provider\n  default: gemini-3.8-flash-high # original model\n  fallback: keep-me\n";
        let updated = apply_sync_losslessly(
            current,
            "http://127.0.0.1:8045/v1",
            "sk-test",
            true,
            &[],
            false,
            None,
            Some(backup),
        )
        .unwrap();
        let parsed = doc(&updated);
        assert_eq!(
            scalar_at(&parsed, "/model/provider").as_deref(),
            Some("openai-codex")
        );
        assert_eq!(
            scalar_at(&parsed, "/model/default").as_deref(),
            Some("gpt-5-codex")
        );
        assert!(updated.contains("provider: openai-codex # original provider"));
    }

    #[test]
    fn deactivation_without_safe_backup_removes_only_managed_selection() {
        let current = "model:\n  provider: custom:antigravity-manager\n  default: managed-model\n  fallback: keep-me\n";
        let updated = apply_sync_losslessly(
            current,
            "http://127.0.0.1:8045/v1",
            "sk-test",
            true,
            &[],
            false,
            None,
            None,
        )
        .unwrap();
        let parsed = doc(&updated);
        assert!(scalar_at(&parsed, "/model/provider").is_none());
        assert!(scalar_at(&parsed, "/model/default").is_none());
        assert_eq!(
            scalar_at(&parsed, "/model/fallback").as_deref(),
            Some("keep-me")
        );
    }

    #[test]
    fn deactivation_restores_model_with_implicit_provider() {
        let backup = "model:\n  default: original-model\n";
        let current = "model:\n  provider: custom:antigravity-manager\n  default: managed-model\n  fallback: keep-me\n";
        let mut parsed = doc(current);
        deactivate(&mut parsed, Some(&doc(backup))).unwrap();
        assert!(scalar_at(&parsed, "/model/provider").is_none());
        assert_eq!(
            scalar_at(&parsed, "/model/default").as_deref(),
            Some("original-model")
        );
        assert_eq!(
            scalar_at(&parsed, "/model/fallback").as_deref(),
            Some("keep-me")
        );
        let restored = doc(&apply_restore_losslessly(
            "model:\n  provider: custom:antigravity-manager\n  default: managed-model\n",
            backup,
        )
        .unwrap());
        assert!(scalar_at(&restored, "/model/provider").is_none());
        assert_eq!(
            scalar_at(&restored, "/model/default").as_deref(),
            Some("original-model")
        );
    }

    #[test]
    fn first_sync_backup_preserves_empty_baseline() {
        let directory = std::env::temp_dir().join(format!("hermes-test-{}", uuid::Uuid::new_v4()));
        let path = directory.join(HERMES_CONFIG_FILE);
        let backup = directory.join(format!("{HERMES_CONFIG_FILE}{BACKUP_SUFFIX}"));
        create_backup(&path).unwrap();
        assert_eq!(fs::read_to_string(&backup).unwrap(), EMPTY_CONFIG);
        let current = apply_sync_losslessly(
            EMPTY_CONFIG,
            "http://127.0.0.1:8045/v1",
            "sk-test",
            true,
            &[],
            true,
            Some("test-model"),
            Some(EMPTY_CONFIG),
        )
        .unwrap();
        atomically_write_source(&path, &current).unwrap();
        create_backup(&path).unwrap();
        let baseline = fs::read_to_string(&backup).unwrap();
        assert_eq!(baseline, EMPTY_CONFIG);
        let restored = doc(&apply_restore_losslessly(&current, &baseline).unwrap());
        assert!(resolve_optional(&restored, "/providers").is_none());
        assert!(resolve_optional(&restored, "/model").is_none());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn clear_preserves_unrelated_bytes() {
        let source = "# top\nproviders:\n  antigravity-manager:\n    api: http://local/v1\n  other: {api: https://example.test/v1} # keep\ndisplay:\n  theme: custom # keep comment\n";
        let (updated, changed) = apply_clear_losslessly(source, None).unwrap();
        assert!(changed);
        assert_eq!(
            updated,
            "# top\nproviders:\n  other: {api: https://example.test/v1} # keep\ndisplay:\n  theme: custom # keep comment\n"
        );
    }

    #[test]
    fn clear_automatically_deactivates_managed_provider_with_backup() {
        let backup = "model:\n  provider: openrouter\n  default: anthropic/claude-3.5-sonnet\nproviders:\n  openrouter:\n    api_key: test\n";
        let source = "model:\n  provider: custom:antigravity-manager\n  default: gemini-2.5-flash\nproviders:\n  antigravity-manager:\n    name: Antigravity Manager\n  openrouter:\n    api_key: test\n";
        let (updated, changed) = apply_clear_losslessly(source, Some(backup)).unwrap();
        assert!(changed);
        let parsed = doc(&updated);
        assert_eq!(
            scalar_at(&parsed, "/model/provider").as_deref(),
            Some("openrouter")
        );
        assert_eq!(
            scalar_at(&parsed, "/model/default").as_deref(),
            Some("anthropic/claude-3.5-sonnet")
        );
        assert!(resolve_optional(&parsed, "/providers/antigravity-manager").is_none());
        assert!(resolve_optional(&parsed, "/providers/openrouter").is_some());
    }

    #[test]
    fn clear_automatically_deactivates_managed_provider_without_backup() {
        let source = "model:\n  provider: custom:antigravity-manager\n  default: gemini-2.5-flash\nproviders:\n  antigravity-manager:\n    name: Antigravity Manager\n";
        let (updated, changed) = apply_clear_losslessly(source, None).unwrap();
        assert!(changed);
        let parsed = doc(&updated);
        assert!(resolve_optional(&parsed, "/model").is_none());
        assert!(resolve_optional(&parsed, "/providers").is_none());
    }

    #[test]
    fn clear_deactivates_managed_provider_retaining_other_model_fields_when_no_backup() {
        let source = "model:\n  provider: custom:antigravity-manager\n  default: gemini-2.5-flash\n  temperature: 0.7\nproviders:\n  antigravity-manager:\n    name: Antigravity Manager\n";
        let (updated, changed) = apply_clear_losslessly(source, None).unwrap();
        assert!(changed);
        let parsed = doc(&updated);
        assert!(resolve_optional(&parsed, "/model/provider").is_none());
        assert!(resolve_optional(&parsed, "/model/default").is_none());
        assert_eq!(
            scalar_at(&parsed, "/model/temperature").as_deref(),
            Some("0.7")
        );
        assert!(resolve_optional(&parsed, "/providers").is_none());
    }

    #[test]
    fn restore_preserves_unrelated_current_changes() {
        let backup = "providers:\n  antigravity-manager:\n    # original provider comment\n    api: https://old.example/v1\n    custom: keep-original\nmodel:\n  provider: openrouter\n  default: old-model\n";
        let current = "providers:\n  antigravity-manager:\n    # original provider comment\n    api: http://127.0.0.1:8045/v1\n    api_key: sk-test\n  other:\n    api: https://new.example/v1 # changed later\n  added-later:\n    api: https://added.example/v1\nmodel:\n  provider: custom:antigravity-manager\n  default: gemini-2.5-pro\n  fallback: keep-me\ndisplay:\n  theme: custom-theme\n";
        let restored = apply_restore_losslessly(current, backup).unwrap();
        let parsed = doc(&restored);
        assert_eq!(
            scalar_at(&parsed, "/providers/antigravity-manager/api").as_deref(),
            Some("https://old.example/v1")
        );
        assert_eq!(
            scalar_at(&parsed, "/providers/other/api").as_deref(),
            Some("https://new.example/v1")
        );
        assert_eq!(
            scalar_at(&parsed, "/model/provider").as_deref(),
            Some("openrouter")
        );
        assert_eq!(
            scalar_at(&parsed, "/model/fallback").as_deref(),
            Some("keep-me")
        );
        assert!(
            restored.contains("# original provider comment"),
            "{restored}"
        );
        assert!(restored.contains("added-later:"));
    }

    #[test]
    fn redaction_preserves_comments_and_formatting() {
        let source = "# credentials\nproviders:\n  antigravity-manager:\n    api_key: 'secret-value' # never expose\n    max_tokens: 4096\ngateway: {telegram_bot_token: bot-secret, enabled: true} # flow\n";
        let redacted = redact_sensitive_source(source).unwrap();
        assert!(redacted.contains("api_key: '[REDACTED]' # never expose"));
        assert!(redacted.contains("max_tokens: 4096"));
        assert!(redacted.contains("telegram_bot_token: [REDACTED]"));
        assert!(redacted.contains("enabled: true} # flow"));
        assert!(!redacted.contains("secret-value"));
        assert!(!redacted.contains("bot-secret"));
    }

    #[test]
    fn sync_preserves_crlf_line_endings() {
        let source = "# windows\r\nproviders:\r\n  other:\r\n    api: https://example.test/v1\r\n";
        let updated = apply_sync_losslessly(
            source,
            "http://127.0.0.1:8045/v1",
            "sk-test",
            true,
            &[],
            false,
            None,
            None,
        )
        .unwrap();
        assert!(!updated.replace("\r\n", "").contains('\n'), "{updated:?}");
        assert!(updated.contains("\r\n  antigravity-manager:\r\n"));
    }

    #[test]
    fn sync_repairs_malformed_managed_shapes() {
        let updated = apply_sync_losslessly(
            "providers: broken\nmodel: keep-me\n",
            "http://127.0.0.1:8045/v1",
            "sk-test",
            true,
            &[],
            true,
            Some("gemini-2.5-pro"),
            None,
        )
        .unwrap();
        let parsed = doc(&updated);
        assert_eq!(
            scalar_at(&parsed, "/providers/antigravity-manager/api").as_deref(),
            Some("http://127.0.0.1:8045/v1")
        );
        assert_eq!(
            scalar_at(&parsed, "/model/provider").as_deref(),
            Some(PROVIDER_REF)
        );
    }

    #[test]
    fn malformed_yaml_is_rejected_before_editing() {
        assert!(apply_sync_losslessly(
            "providers: [broken\n",
            "http://127.0.0.1:8045/v1",
            "sk-test",
            true,
            &[],
            false,
            None,
            None,
        )
        .is_err());
    }

    #[test]
    fn normalize_base_url_appends_v1_once() {
        assert_eq!(
            normalize_base_url("http://127.0.0.1:8045"),
            "http://127.0.0.1:8045/v1"
        );
        assert_eq!(
            normalize_base_url("http://127.0.0.1:8045/v1/"),
            "http://127.0.0.1:8045/v1"
        );
    }
}
