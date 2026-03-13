use super::{
    collect_existing_bootflow_hint_keys, is_activity_handler_selector, is_bootstrap_selector,
    is_deeplink_selector, is_main_like_selector, is_runapp_selector,
    library_is_bootstrap_context, normalize_method_selector, owner_is_bootstrap_context,
    push_synthetic_hint, SyntheticHintInput,
};
use flutterdec_adapter::ProgramModel;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

const RES_STRING_POOL_TYPE: u16 = 0x0001;
const RES_XML_TYPE: u16 = 0x0003;
const RES_XML_START_ELEMENT_TYPE: u16 = 0x0102;
const RES_XML_END_ELEMENT_TYPE: u16 = 0x0103;
const TYPE_STRING: u8 = 0x03;
const TYPE_INT_BOOLEAN: u8 = 0x12;
const NO_ENTRY_U32: u32 = 0xffff_ffff;

#[derive(Debug, Clone, Default)]
pub(super) struct AndroidManifestSignals {
    pub(super) package_name: Option<String>,
    pub(super) has_main_launcher: bool,
    pub(super) has_view_browsable: bool,
    pub(super) activities: Vec<String>,
    pub(super) deeplink_entries: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) struct AndroidManifestConfidence {
    pub(super) package_name: String,
    pub(super) launcher: String,
    pub(super) deeplink: String,
    pub(super) activities: String,
}

impl Default for AndroidManifestConfidence {
    fn default() -> Self {
        Self {
            package_name: "none".to_string(),
            launcher: "none".to_string(),
            deeplink: "none".to_string(),
            activities: "none".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct AndroidManifestInspection {
    pub(super) present: bool,
    pub(super) parse_mode: String,
    pub(super) parse_error: Option<String>,
    pub(super) confidence: AndroidManifestConfidence,
    pub(super) signals: AndroidManifestSignals,
}

impl Default for AndroidManifestInspection {
    fn default() -> Self {
        Self {
            present: false,
            parse_mode: "none".to_string(),
            parse_error: None,
            confidence: AndroidManifestConfidence::default(),
            signals: AndroidManifestSignals::default(),
        }
    }
}

fn normalize_activity_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let tail = trimmed.rsplit('.').next().unwrap_or(trimmed).trim();
    if tail.is_empty() {
        return None;
    }
    Some(tail.to_ascii_lowercase())
}

fn class_matches_manifest_activity(owner_class: &str, activities: &HashSet<String>) -> bool {
    let owner = owner_class.trim();
    if owner.is_empty() {
        return false;
    }
    let tail = owner.rsplit(['.', '$']).next().unwrap_or(owner).to_ascii_lowercase();
    activities.contains(&tail)
}

fn read_u16_le(bytes: &[u8], at: usize) -> Option<u16> {
    let b0 = *bytes.get(at)?;
    let b1 = *bytes.get(at + 1)?;
    Some(u16::from_le_bytes([b0, b1]))
}

fn read_u32_le(bytes: &[u8], at: usize) -> Option<u32> {
    let b0 = *bytes.get(at)?;
    let b1 = *bytes.get(at + 1)?;
    let b2 = *bytes.get(at + 2)?;
    let b3 = *bytes.get(at + 3)?;
    Some(u32::from_le_bytes([b0, b1, b2, b3]))
}

fn decode_utf8_len(bytes: &[u8], at: usize) -> Option<(usize, usize)> {
    let first = *bytes.get(at)?;
    if (first & 0x80) == 0 {
        Some((first as usize, 1))
    } else {
        let second = *bytes.get(at + 1)?;
        let len = (((first as usize) & 0x7f) << 8) | (second as usize);
        Some((len, 2))
    }
}

fn decode_utf16_len(bytes: &[u8], at: usize) -> Option<(usize, usize)> {
    let first = read_u16_le(bytes, at)?;
    if (first & 0x8000) == 0 {
        Some((first as usize, 2))
    } else {
        let second = read_u16_le(bytes, at + 2)?;
        let len = ((((first as usize) & 0x7fff) << 16) | (second as usize)) as usize;
        Some((len, 4))
    }
}

fn decode_string_pool_entry(bytes: &[u8], at: usize, utf8: bool) -> Option<String> {
    if utf8 {
        let (_, utf16_len_size) = decode_utf8_len(bytes, at)?;
        let start_utf8_len = at + utf16_len_size;
        let (utf8_len, utf8_len_size) = decode_utf8_len(bytes, start_utf8_len)?;
        let str_start = start_utf8_len + utf8_len_size;
        let str_end = str_start + utf8_len;
        let raw = bytes.get(str_start..str_end)?;
        return Some(String::from_utf8_lossy(raw).to_string());
    }
    let (utf16_len, utf16_len_size) = decode_utf16_len(bytes, at)?;
    let str_start = at + utf16_len_size;
    let mut out = Vec::with_capacity(utf16_len);
    for i in 0..utf16_len {
        let ch = read_u16_le(bytes, str_start + i * 2)?;
        out.push(ch);
    }
    Some(String::from_utf16_lossy(&out))
}

fn parse_string_pool_chunk(bytes: &[u8], offset: usize, header_size: usize, size: usize) -> Option<Vec<String>> {
    if header_size < 28 || offset + size > bytes.len() {
        return None;
    }
    let string_count = read_u32_le(bytes, offset + 8)? as usize;
    let flags = read_u32_le(bytes, offset + 16)?;
    let strings_start = read_u32_le(bytes, offset + 20)? as usize;
    let utf8 = (flags & (1 << 8)) != 0;

    let offsets_base = offset + header_size;
    let strings_base = offset + strings_start;
    let mut out = Vec::with_capacity(string_count);
    for i in 0..string_count {
        let str_off = read_u32_le(bytes, offsets_base + i * 4)? as usize;
        let at = strings_base + str_off;
        let s = decode_string_pool_entry(bytes, at, utf8)?;
        out.push(s);
    }
    Some(out)
}

fn string_from_pool(pool: &[String], idx: u32) -> Option<String> {
    if idx == NO_ENTRY_U32 {
        return None;
    }
    pool.get(idx as usize).cloned()
}

fn decode_typed_attr_value(
    pool: &[String],
    raw_value_idx: u32,
    value_type: u8,
    value_data: u32,
) -> Option<String> {
    if raw_value_idx != NO_ENTRY_U32 {
        return string_from_pool(pool, raw_value_idx);
    }
    match value_type {
        TYPE_STRING => string_from_pool(pool, value_data),
        TYPE_INT_BOOLEAN => Some(if value_data != 0 { "true" } else { "false" }.to_string()),
        _ => None,
    }
}

#[derive(Debug, Default, Clone)]
struct ParsedIntentFilter {
    actions: HashSet<String>,
    categories: HashSet<String>,
    data_entries: Vec<String>,
}

#[derive(Debug, Default, Clone)]
struct ParsedActivity {
    name: String,
    filters: Vec<ParsedIntentFilter>,
}

#[derive(Debug, Default, Clone)]
struct ParsedBinaryManifest {
    package_name: Option<String>,
    activities: Vec<ParsedActivity>,
}

fn fully_qualify_component_name(package_name: Option<&str>, raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return String::new();
    }
    if t.starts_with('.') {
        if let Some(pkg) = package_name {
            return format!("{pkg}{t}");
        }
        return t.to_string();
    }
    if t.contains('.') {
        return t.to_string();
    }
    if let Some(pkg) = package_name {
        return format!("{pkg}.{t}");
    }
    t.to_string()
}

fn parse_binary_android_manifest(bytes: &[u8]) -> Result<ParsedBinaryManifest, String> {
    let root_type = read_u16_le(bytes, 0).ok_or("manifest too short")?;
    if root_type != RES_XML_TYPE {
        return Err("not binary android xml".to_string());
    }
    let mut offset = 0usize;
    let mut xml_end = bytes.len();
    let mut pool: Vec<String> = Vec::new();
    let mut parsed = ParsedBinaryManifest::default();
    let mut element_stack: Vec<String> = Vec::new();
    let mut activity_stack: Vec<Option<usize>> = Vec::new();
    let mut filter_stack: Vec<Option<usize>> = Vec::new();

    while offset + 8 <= xml_end {
        let chunk_type = read_u16_le(bytes, offset).ok_or("invalid chunk type")?;
        let header_size = read_u16_le(bytes, offset + 2).ok_or("invalid chunk header")? as usize;
        let chunk_size = read_u32_le(bytes, offset + 4).ok_or("invalid chunk size")? as usize;
        if chunk_size == 0 || offset + chunk_size > xml_end || header_size > chunk_size {
            break;
        }

        if chunk_type == RES_XML_TYPE && offset == 0 {
            if header_size == 0 || chunk_size > bytes.len() {
                return Err("invalid root xml chunk".to_string());
            }
            xml_end = chunk_size;
            offset += header_size;
            continue;
        }

        if chunk_type == RES_STRING_POOL_TYPE {
            if let Some(parsed_pool) = parse_string_pool_chunk(bytes, offset, header_size, chunk_size) {
                pool = parsed_pool;
            }
        } else if chunk_type == RES_XML_START_ELEMENT_TYPE {
            if pool.is_empty() || header_size < 16 || header_size + 20 > chunk_size {
                offset += chunk_size;
                continue;
            }
            let ext_offset = offset + header_size;
            let name_idx = read_u32_le(bytes, ext_offset + 4).unwrap_or(NO_ENTRY_U32);
            let attr_start = read_u16_le(bytes, ext_offset + 8).unwrap_or(0) as usize;
            let attr_size = read_u16_le(bytes, ext_offset + 10).unwrap_or(20) as usize;
            let attr_count = read_u16_le(bytes, ext_offset + 12).unwrap_or(0) as usize;
            let element_name = string_from_pool(&pool, name_idx).unwrap_or_default();

            let attrs_base = ext_offset + attr_start;
            let mut attrs = HashMap::<String, String>::new();
            for i in 0..attr_count {
                let at = attrs_base + i * attr_size;
                if at + 20 > offset + chunk_size {
                    break;
                }
                let attr_name_idx = read_u32_le(bytes, at + 4).unwrap_or(NO_ENTRY_U32);
                let raw_value_idx = read_u32_le(bytes, at + 8).unwrap_or(NO_ENTRY_U32);
                let value_type = *bytes.get(at + 15).unwrap_or(&0);
                let value_data = read_u32_le(bytes, at + 16).unwrap_or(0);
                let Some(attr_name) = string_from_pool(&pool, attr_name_idx) else {
                    continue;
                };
                let Some(attr_value) =
                    decode_typed_attr_value(&pool, raw_value_idx, value_type, value_data)
                else {
                    continue;
                };
                attrs.insert(attr_name, attr_value);
            }

            let parent_activity_ix = activity_stack.last().copied().flatten();
            let parent_filter_ix = filter_stack.last().copied().flatten();
            let mut current_activity_ix = None;
            let mut current_filter_ix = None;

            match element_name.as_str() {
                "manifest" => {
                    if let Some(pkg) = attrs.get("package") {
                        parsed.package_name = Some(pkg.trim().to_string());
                    }
                }
                "activity" | "activity-alias" => {
                    if let Some(name) = attrs.get("name").or_else(|| attrs.get("targetActivity")) {
                        let fq = fully_qualify_component_name(parsed.package_name.as_deref(), name);
                        if !fq.is_empty() {
                            parsed.activities.push(ParsedActivity {
                                name: fq,
                                filters: Vec::new(),
                            });
                            current_activity_ix = Some(parsed.activities.len() - 1);
                        }
                    }
                }
                "intent-filter" => {
                    if let Some(activity_ix) = parent_activity_ix {
                        if let Some(activity) = parsed.activities.get_mut(activity_ix) {
                            activity.filters.push(ParsedIntentFilter::default());
                            current_activity_ix = Some(activity_ix);
                            current_filter_ix = Some(activity.filters.len() - 1);
                        }
                    }
                }
                "action" => {
                    if let (Some(activity_ix), Some(filter_ix)) = (parent_activity_ix, parent_filter_ix)
                    {
                        if let Some(action) = attrs.get("name") {
                            if let Some(filter) = parsed
                                .activities
                                .get_mut(activity_ix)
                                .and_then(|a| a.filters.get_mut(filter_ix))
                            {
                                filter.actions.insert(action.trim().to_ascii_lowercase());
                            }
                        }
                    }
                }
                "category" => {
                    if let (Some(activity_ix), Some(filter_ix)) = (parent_activity_ix, parent_filter_ix)
                    {
                        if let Some(category) = attrs.get("name") {
                            if let Some(filter) = parsed
                                .activities
                                .get_mut(activity_ix)
                                .and_then(|a| a.filters.get_mut(filter_ix))
                            {
                                filter.categories.insert(category.trim().to_ascii_lowercase());
                            }
                        }
                    }
                }
                "data" => {
                    if let (Some(activity_ix), Some(filter_ix)) = (parent_activity_ix, parent_filter_ix)
                    {
                        if let Some(filter) = parsed
                            .activities
                            .get_mut(activity_ix)
                            .and_then(|a| a.filters.get_mut(filter_ix))
                        {
                            let scheme = attrs.get("scheme").map(|v| v.trim()).unwrap_or("");
                            let host = attrs.get("host").map(|v| v.trim()).unwrap_or("");
                            let path = attrs
                                .get("path")
                                .or_else(|| attrs.get("pathPrefix"))
                                .or_else(|| attrs.get("pathPattern"))
                                .map(|v| v.trim())
                                .unwrap_or("");
                            let mut entry = String::new();
                            if !scheme.is_empty() {
                                entry.push_str(scheme);
                                entry.push_str("://");
                            }
                            if !host.is_empty() {
                                entry.push_str(host);
                            }
                            if !path.is_empty() {
                                if !path.starts_with('/') && !entry.ends_with('/') {
                                    entry.push('/');
                                }
                                entry.push_str(path);
                            }
                            if entry.is_empty() {
                                if let Some(mime) = attrs.get("mimeType") {
                                    entry = mime.trim().to_string();
                                }
                            }
                            if !entry.is_empty() {
                                filter.data_entries.push(entry);
                            }
                        }
                    }
                }
                _ => {}
            }

            if current_activity_ix.is_none() {
                current_activity_ix = parent_activity_ix;
            }
            if current_filter_ix.is_none() {
                current_filter_ix = parent_filter_ix;
            }
            element_stack.push(element_name);
            activity_stack.push(current_activity_ix);
            filter_stack.push(current_filter_ix);
        } else if chunk_type == RES_XML_END_ELEMENT_TYPE {
            if !element_stack.is_empty() {
                element_stack.pop();
            }
            if !activity_stack.is_empty() {
                activity_stack.pop();
            }
            if !filter_stack.is_empty() {
                filter_stack.pop();
            }
        }

        offset += chunk_size;
    }

    if parsed.package_name.is_none() && parsed.activities.is_empty() {
        return Err("binary xml parse produced no manifest signals".to_string());
    }
    Ok(parsed)
}

fn signals_from_parsed_binary_manifest(parsed: &ParsedBinaryManifest) -> AndroidManifestSignals {
    let mut has_main_launcher = false;
    let mut has_view_browsable = false;
    let mut deeplink_entries = BTreeSet::new();
    let mut activity_names = BTreeSet::new();

    for activity in &parsed.activities {
        if !activity.name.trim().is_empty() {
            activity_names.insert(activity.name.trim().to_string());
        }
        for filter in &activity.filters {
            let has_main = filter.actions.contains("android.intent.action.main");
            let has_launcher = filter.categories.contains("android.intent.category.launcher");
            if has_main && has_launcher {
                has_main_launcher = true;
            }

            let has_view = filter.actions.contains("android.intent.action.view");
            let has_browsable = filter.categories.contains("android.intent.category.browsable");
            if has_view && has_browsable {
                has_view_browsable = true;
            }
            if has_view {
                deeplink_entries.insert("android.intent.action.VIEW".to_string());
            }
            for entry in &filter.data_entries {
                let t = entry.trim();
                if !t.is_empty() {
                    deeplink_entries.insert(t.to_string());
                }
            }
        }
    }

    AndroidManifestSignals {
        package_name: parsed
            .package_name
            .as_ref()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .filter(|pkg| !looks_non_app_package(pkg)),
        has_main_launcher,
        has_view_browsable,
        activities: activity_names.into_iter().take(40).collect(),
        deeplink_entries: deeplink_entries.into_iter().take(40).collect(),
    }
}

fn confidence_for_signals(signals: &AndroidManifestSignals, structured: bool) -> AndroidManifestConfidence {
    AndroidManifestConfidence {
        package_name: if signals.package_name.is_some() {
            if structured {
                "high".to_string()
            } else {
                "medium".to_string()
            }
        } else {
            "low".to_string()
        },
        launcher: if signals.has_main_launcher {
            if structured {
                "high".to_string()
            } else {
                "medium".to_string()
            }
        } else {
            "low".to_string()
        },
        deeplink: if signals.has_view_browsable || !signals.deeplink_entries.is_empty() {
            if structured {
                "high".to_string()
            } else {
                "medium".to_string()
            }
        } else {
            "low".to_string()
        },
        activities: if !signals.activities.is_empty() {
            if structured {
                "high".to_string()
            } else {
                "medium".to_string()
            }
        } else {
            "low".to_string()
        },
    }
}

fn collect_ascii_strings(bytes: &[u8], min_len: usize, max_items: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for b in bytes {
        if (0x20..=0x7e).contains(b) {
            cur.push(*b);
            continue;
        }
        if cur.len() >= min_len {
            out.push(String::from_utf8_lossy(&cur).to_string());
            if out.len() >= max_items {
                return out;
            }
        }
        cur.clear();
    }
    if cur.len() >= min_len && out.len() < max_items {
        out.push(String::from_utf8_lossy(&cur).to_string());
    }
    out
}

fn collect_utf16le_ascii_like_strings(bytes: &[u8], min_len: usize, max_items: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        let mut j = i;
        let mut cur = Vec::new();
        while j + 1 < bytes.len() {
            let lo = bytes[j];
            let hi = bytes[j + 1];
            if hi != 0 || !(0x20..=0x7e).contains(&lo) {
                break;
            }
            cur.push(lo);
            j += 2;
        }
        if cur.len() >= min_len {
            out.push(String::from_utf8_lossy(&cur).to_string());
            if out.len() >= max_items {
                return out;
            }
            i = j + 2;
        } else {
            i += 1;
        }
    }
    out
}

fn collect_manifest_strings(bytes: &[u8]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for s in collect_ascii_strings(bytes, 4, 30_000) {
        let t = s.trim();
        if !t.is_empty() {
            out.insert(t.to_string());
        }
    }
    for s in collect_utf16le_ascii_like_strings(bytes, 4, 30_000) {
        let t = s.trim();
        if !t.is_empty() {
            out.insert(t.to_string());
        }
    }
    out
}

fn sanitize_manifest_token(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let start = trimmed
        .find(|c: char| c.is_ascii_alphabetic() || c == '.')
        .unwrap_or(trimmed.len());
    if start >= trimmed.len() {
        return None;
    }
    let candidate = &trimmed[start..];
    let candidate = candidate.trim_matches(|c: char| {
        !c.is_ascii_alphanumeric() && c != '.' && c != '_' && c != '$' && c != ':'
    });
    if candidate.len() < 3 || candidate.contains(' ') {
        return None;
    }
    let mut cleaned = candidate.to_string();
    if cleaned.len() > 4 {
        let tail = cleaned.get(1..).unwrap_or_default();
        let noisy_prefixed = tail.starts_with("com.")
            || tail.starts_with("org.")
            || tail.starts_with("io.")
            || tail.starts_with("androidx.")
            || tail.starts_with("net.")
            || tail.starts_with("app.")
            || tail.starts_with("dev.")
            || tail.starts_with("me.")
            || tail.starts_with("oss.");
        if noisy_prefixed {
            cleaned = tail.to_string();
        }
    }
    Some(cleaned)
}

fn is_probable_package_name(token: &str) -> bool {
    let is_lower_package = token.split('.').all(|seg| {
        !seg.is_empty()
            && seg
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    });
    if !is_lower_package {
        return false;
    }
    let segment_count = token.split('.').count();
    if !(3..=5).contains(&segment_count) {
        return false;
    }
    if !token.contains('.') || token.contains('/') || token.contains(' ') {
        return false;
    }
    let lower = token.to_ascii_lowercase();
    !(lower.starts_with("android.")
        || lower.starts_with("kotlin.")
        || lower.starts_with("java.")
        || lower.starts_with("com.google.")
        || lower.starts_with("io.flutter.")
        || lower.starts_with("android.intent.")
        || lower.contains("intent.action")
        || lower.contains("intent.category"))
}

fn extract_manifest_attr_value(raw: &str, attr_name: &str) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    let needle = format!("{}=", attr_name.to_ascii_lowercase());
    let start = lower.find(&needle)?;
    let mut tail = raw[start + needle.len()..].trim_start();
    if tail.is_empty() {
        return None;
    }

    let value = if let Some(quote) = tail.chars().next().filter(|c| *c == '"' || *c == '\'') {
        tail = &tail[quote.len_utf8()..];
        let end = tail.find(quote)?;
        &tail[..end]
    } else {
        let end = tail
            .find(|c: char| {
                c.is_ascii_whitespace()
                    || matches!(c, '/' | '>' | '"' | '\'')
                    || c == '\0'
            })
            .unwrap_or(tail.len());
        &tail[..end]
    };

    let cleaned = value.trim();
    if cleaned.is_empty() {
        return None;
    }
    Some(cleaned.to_string())
}

fn extract_manifest_package_attr(raw: &str) -> Option<String> {
    let candidate = extract_manifest_attr_value(raw, "package")?.to_ascii_lowercase();
    if candidate.is_empty() || !is_probable_package_name(&candidate) {
        return None;
    }
    Some(candidate)
}

fn infer_package_name(strings: &BTreeSet<String>, activities: &[String]) -> Option<String> {
    for activity in activities {
        let t = activity.trim();
        if t.starts_with('.') {
            continue;
        }
        if !t.ends_with("MainActivity") {
            continue;
        }
        let parts = t.split('.').collect::<Vec<_>>();
        if parts.len() < 2 {
            continue;
        }
        let package = parts[..parts.len().saturating_sub(1)].join(".");
        let package_lower = package.to_ascii_lowercase();
        if package_lower.starts_with("android.")
            || package_lower.starts_with("io.flutter.")
            || package_lower.starts_with("androidx.")
        {
            continue;
        }
        return Some(package);
    }

    for s in strings {
        if let Some(pkg) = extract_manifest_package_attr(s) {
            return Some(pkg);
        }
    }

    strings.iter().find_map(|s| {
        let token = sanitize_manifest_token(s)?;
        if !is_probable_package_name(&token) {
            return None;
        }
        Some(token)
    })
}

fn infer_activity_names(strings: &BTreeSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    for s in strings {
        let extracted = extract_manifest_attr_value(s, "android:name")
            .or_else(|| extract_manifest_attr_value(s, "name"))
            .or_else(|| extract_manifest_attr_value(s, "targetactivity"));
        if let Some(activity_name) = extracted {
            let trimmed = activity_name.trim();
            if !trimmed.is_empty() && !trimmed.contains(' ') && trimmed.contains("Activity") {
                let lower = trimmed.to_ascii_lowercase();
                if !lower.starts_with("android.") && !lower.starts_with("io.flutter.") {
                    out.push(trimmed.to_string());
                    continue;
                }
            }
        }

        let Some(t) = sanitize_manifest_token(s) else {
            continue;
        };
        if t.is_empty() || t.contains('/') || t.contains(' ') {
            continue;
        }
        if !t.contains("Activity") {
            continue;
        }
        let lower = t.to_ascii_lowercase();
        if lower.starts_with("android.") || lower.starts_with("io.flutter.") {
            continue;
        }
        out.push(t);
    }
    out.sort();
    out.dedup();
    out.truncate(30);
    out
}

fn infer_deeplink_entries(strings: &BTreeSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    for s in strings {
        let Some(t) = sanitize_manifest_token(s) else {
            continue;
        };
        if t.is_empty() || t.len() > 180 {
            continue;
        }
        let lower = t.to_ascii_lowercase();
        if lower.contains("schemas.android.com/apk/res/android") {
            continue;
        }
        if lower.contains("://") || lower.starts_with("android.intent.action.view") {
            out.push(t);
        }
    }
    out.sort();
    out.dedup();
    out.truncate(30);
    out
}

fn looks_non_app_package(package: &str) -> bool {
    let lower = package.to_ascii_lowercase();
    lower.starts_with("android.")
        || lower.starts_with("androidx.")
        || lower.starts_with("com.google.")
        || lower.starts_with("io.flutter.")
        || lower.starts_with("com.pichillilorenzo.")
        || lower.starts_with("com.ryanheise.")
}

fn analyze_manifest_bytes(bytes: &[u8]) -> AndroidManifestSignals {
    let strings = collect_manifest_strings(bytes);
    let lower = strings
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let contains_manifest_token = |needle: &str| {
        lower
            .iter()
            .any(|entry| entry == needle || entry.contains(needle))
    };
    let has_main_launcher = contains_manifest_token("android.intent.action.main")
        && contains_manifest_token("android.intent.category.launcher");
    let deeplink_entries = infer_deeplink_entries(&strings);
    let has_custom_deeplink = deeplink_entries
        .iter()
        .any(|v| v.contains("://") && !v.to_ascii_lowercase().starts_with("http"));
    let has_view_browsable = contains_manifest_token("android.intent.action.view")
        && (contains_manifest_token("android.intent.category.browsable") || has_custom_deeplink);
    let activities = infer_activity_names(&strings);
    let package_name = infer_package_name(&strings, &activities)
        .filter(|pkg| !looks_non_app_package(pkg));

    AndroidManifestSignals {
        package_name,
        has_main_launcher,
        has_view_browsable,
        activities,
        deeplink_entries,
    }
}

fn read_android_manifest_from_apk(input_path: &Path) -> Result<Option<Vec<u8>>, String> {
    let f = fs::File::open(input_path).map_err(|e| format!("open apk: {e}"))?;
    let mut zip = ZipArchive::new(f).map_err(|e| format!("parse apk zip: {e}"))?;
    for path in ["AndroidManifest.xml", "base/AndroidManifest.xml"] {
        if let Ok(mut entry) = zip.by_name(path) {
            let mut out = Vec::new();
            entry
                .read_to_end(&mut out)
                .map_err(|e| format!("read manifest entry {path}: {e}"))?;
            return Ok(Some(out));
        }
    }
    for i in 0..zip.len() {
        let Ok(mut entry) = zip.by_index(i) else {
            continue;
        };
        if !entry.name().ends_with("/AndroidManifest.xml") {
            continue;
        }
        let mut out = Vec::new();
        entry
            .read_to_end(&mut out)
            .map_err(|e| format!("read manifest entry by index: {e}"))?;
        return Ok(Some(out));
    }
    Ok(None)
}

pub(super) fn inspect_android_manifest(input_path: &Path) -> AndroidManifestInspection {
    let is_apk = input_path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("apk"));
    if !is_apk {
        return AndroidManifestInspection {
            parse_mode: "not_apk".to_string(),
            ..AndroidManifestInspection::default()
        };
    }
    match read_android_manifest_from_apk(input_path) {
        Ok(Some(bytes)) => inspect_manifest_bytes(&bytes),
        Ok(None) => AndroidManifestInspection {
            present: false,
            parse_mode: "missing".to_string(),
            parse_error: None,
            confidence: AndroidManifestConfidence::default(),
            signals: AndroidManifestSignals::default(),
        },
        Err(err) => AndroidManifestInspection {
            present: false,
            parse_mode: "read_error".to_string(),
            parse_error: Some(err),
            confidence: AndroidManifestConfidence::default(),
            signals: AndroidManifestSignals::default(),
        },
    }
}

fn inspect_manifest_bytes(bytes: &[u8]) -> AndroidManifestInspection {
    match parse_binary_android_manifest(bytes) {
        Ok(parsed) => {
            let signals = signals_from_parsed_binary_manifest(&parsed);
            AndroidManifestInspection {
                present: true,
                parse_mode: "binary_axml".to_string(),
                parse_error: None,
                confidence: confidence_for_signals(&signals, true),
                signals,
            }
        }
        Err(binary_err) => {
            let signals = analyze_manifest_bytes(bytes);
            AndroidManifestInspection {
                present: true,
                parse_mode: "heuristic_strings".to_string(),
                parse_error: Some(format!("binary_axml_parse_failed: {binary_err}")),
                confidence: confidence_for_signals(&signals, false),
                signals,
            }
        }
    }
}

pub(super) fn enrich_model_with_manifest_bootflow_hints(
    model: &ProgramModel,
    signals: &AndroidManifestSignals,
) -> (ProgramModel, usize) {
    let mut enriched = model.clone();
    let mut inserted = 0usize;
    let mut class_library = HashMap::new();
    for class in &enriched.classes {
        class_library
            .entry(class.name.clone())
            .or_insert_with(|| class.library_uri.clone());
    }

    let mut seen = collect_existing_bootflow_hint_keys(&enriched);

    let activity_set = signals
        .activities
        .iter()
        .filter_map(|name| normalize_activity_name(name))
        .collect::<HashSet<_>>();
    let has_deeplink_signal = signals.has_view_browsable || !signals.deeplink_entries.is_empty();

    let functions = enriched.functions.clone();
    for function in functions {
        let selector = normalize_method_selector(&function.name);
        if selector.is_empty() {
            continue;
        }
        let owner = function.owner_class.trim();
        let owner_lower = owner.to_ascii_lowercase();
        let library_uri = class_library
            .get(&function.owner_class)
            .cloned()
            .unwrap_or_default();
        let library_lower = library_uri.to_ascii_lowercase();

        if signals.has_main_launcher
            && is_main_like_selector(&selector)
            && push_synthetic_hint(
                &mut enriched,
                &mut seen,
                &SyntheticHintInput {
                    decoded_kind: "ManifestMainCandidate",
                    selector: &selector,
                    target_va: Some(function.entry_va),
                    owner_class: owner,
                    library_uri: &library_uri,
                    value: "manifest:main-launcher",
                    confidence: Some(0.95),
                    source: Some("manifest"),
                },
            )
        {
                inserted += 1;
        }
        if signals.has_main_launcher
            && is_runapp_selector(&selector)
            && push_synthetic_hint(
                &mut enriched,
                &mut seen,
                &SyntheticHintInput {
                    decoded_kind: "ManifestRunAppCandidate",
                    selector: &selector,
                    target_va: Some(function.entry_va),
                    owner_class: owner,
                    library_uri: &library_uri,
                    value: "manifest:runapp",
                    confidence: Some(0.95),
                    source: Some("manifest"),
                },
            )
        {
                inserted += 1;
        }
        if has_deeplink_signal
            && is_deeplink_selector(&selector)
            && push_synthetic_hint(
                &mut enriched,
                &mut seen,
                &SyntheticHintInput {
                    decoded_kind: "ManifestDeepLinkCandidate",
                    selector: &selector,
                    target_va: Some(function.entry_va),
                    owner_class: owner,
                    library_uri: &library_uri,
                    value: "manifest:deeplink",
                    confidence: Some(0.9),
                    source: Some("manifest"),
                },
            )
        {
                inserted += 1;
        }
        if has_deeplink_signal
            && class_matches_manifest_activity(owner, &activity_set)
            && is_activity_handler_selector(&selector)
            && push_synthetic_hint(
                &mut enriched,
                &mut seen,
                &SyntheticHintInput {
                    decoded_kind: "ManifestActivityCandidate",
                    selector: &selector,
                    target_va: Some(function.entry_va),
                    owner_class: owner,
                    library_uri: &library_uri,
                    value: "manifest:activity",
                    confidence: Some(0.9),
                    source: Some("manifest"),
                },
            )
        {
            inserted += 1;
        }
        if signals.has_main_launcher
            && is_bootstrap_selector(&selector)
            && (owner_is_bootstrap_context(&owner_lower)
                || library_is_bootstrap_context(&library_lower))
            && push_synthetic_hint(
                &mut enriched,
                &mut seen,
                &SyntheticHintInput {
                    decoded_kind: "ManifestBootstrapCandidate",
                    selector: &selector,
                    target_va: Some(function.entry_va),
                    owner_class: owner,
                    library_uri: &library_uri,
                    value: "manifest:bootstrap",
                    confidence: Some(0.9),
                    source: Some("manifest"),
                },
            )
        {
            inserted += 1;
        }
    }

    (enriched, inserted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_scores_reflect_parse_quality() {
        let signals = AndroidManifestSignals {
            package_name: Some("org.localsend.localsend_app".to_string()),
            has_main_launcher: true,
            has_view_browsable: true,
            activities: vec![".MainActivity".to_string()],
            deeplink_entries: vec!["localsend://share".to_string()],
        };

        let structured = confidence_for_signals(&signals, true);
        assert_eq!(structured.package_name, "high");
        assert_eq!(structured.launcher, "high");
        assert_eq!(structured.deeplink, "high");
        assert_eq!(structured.activities, "high");

        let heuristic = confidence_for_signals(&signals, false);
        assert_eq!(heuristic.package_name, "medium");
        assert_eq!(heuristic.launcher, "medium");
        assert_eq!(heuristic.deeplink, "medium");
        assert_eq!(heuristic.activities, "medium");
    }

    #[test]
    fn inspect_manifest_bytes_uses_heuristic_fallback_for_plaintext_xml() {
        let manifest_text = br#"
            <manifest package="org.localsend.localsend_app">
              <application>
                <activity android:name=".MainActivity">
                  <intent-filter>
                    <action android:name="android.intent.action.MAIN" />
                    <category android:name="android.intent.category.LAUNCHER" />
                  </intent-filter>
                  <intent-filter>
                    <action android:name="android.intent.action.VIEW" />
                    <category android:name="android.intent.category.BROWSABLE" />
                    <data android:scheme="localsend" android:host="share" />
                  </intent-filter>
                </activity>
              </application>
            </manifest>
        "#;

        let inspection = inspect_manifest_bytes(manifest_text);
        assert!(inspection.present);
        assert_eq!(inspection.parse_mode, "heuristic_strings");
        assert!(
            inspection
                .parse_error
                .as_deref()
                .is_some_and(|v| v.starts_with("binary_axml_parse_failed:"))
        );
        assert_eq!(
            inspection.signals.package_name.as_deref(),
            Some("org.localsend.localsend_app")
        );
        assert!(inspection.signals.has_main_launcher);
        assert!(inspection.signals.has_view_browsable);
        assert!(
            inspection
                .signals
                .activities
                .iter()
                .any(|v| v.contains("MainActivity"))
        );
        assert_eq!(inspection.confidence.package_name, "medium");
    }

    #[test]
    fn inspect_android_manifest_non_apk_marks_parse_mode() {
        let inspection = inspect_android_manifest(Path::new("/tmp/libapp.so"));
        assert!(!inspection.present);
        assert_eq!(inspection.parse_mode, "not_apk");
        assert!(inspection.parse_error.is_none());
        assert_eq!(inspection.confidence.package_name, "none");
    }
}
