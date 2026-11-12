// Project-shaped output layout for `flutterdec decompile`.
//
// This file is included into the crate root beside the rest of the pipeline.
// It owns path planning and index generation; it does not change the emitted
// pseudocode or the emitter's symbol rules.

const MAX_COMPONENT_BYTES: usize = 120;
const MAX_RELATIVE_PATH_BYTES: usize = 3_500;
const LAYOUT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Placement {
    Model,
    None,
    Heuristic,
}

#[derive(Debug, Clone)]
struct LayoutFunction {
    function_id: u64,
    entry_va: u64,
    /// The symbol the emitter used. Always present, even when the model has no
    /// name and the emitter rendered `fn_0x<va>`.
    emitted_name: String,
    /// The model's recovered bare name, before canonical rewriting.
    model_name: Option<String>,
    class: Option<String>,
    class_provenance: Option<Provenance>,
    library: Option<String>,
    library_provenance: Option<Provenance>,
    function_provenance: Option<Provenance>,
    placement: Placement,
}

#[derive(Debug, Clone, Serialize)]
struct LayoutEntry {
    function_id: u64,
    path: String,
    name: String,
    model_name: Option<String>,
    class: Option<String>,
    library: Option<String>,
    entry_va: String,
    function_provenance: Option<&'static str>,
    class_provenance: Option<&'static str>,
    library_provenance: Option<&'static str>,
    placement: Placement,
    collision: bool,
    /// Per-function path used for asm and IR artifacts. In library layout this
    /// is the Phase A path even though `path` names the composed library file.
    #[serde(skip)]
    detail_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    line_start: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line_end: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    declared_name: Option<String>,
}

#[derive(Debug, Clone)]
struct LayoutPlan {
    root_package: Option<String>,
    root_package_source: &'static str,
    entries: Vec<LayoutEntry>,
    collisions: usize,
    unattributed: usize,
}

#[derive(Debug, Clone)]
struct OutputLayoutSummary {
    layout: &'static str,
    root_package: Option<String>,
    root_package_source: &'static str,
    source_files: usize,
    libraries: usize,
    unattributed: usize,
    path_collisions: usize,
}

impl OutputLayoutSummary {
    fn to_json(&self) -> serde_json::Value {
        json!({
            "layout": self.layout,
            "layout_version": LAYOUT_VERSION,
            "root_package": self.root_package.as_deref(),
            "root_package_source": self.root_package_source,
            "source_files": self.source_files,
            "libraries": self.libraries,
            "unattributed_functions": self.unattributed,
            "path_collisions": self.path_collisions,
            "index_path": "index.json"
        })
    }
}

#[derive(Serialize)]
struct OutputIndex<'a> {
    schema_version: u32,
    layout: &'static str,
    layout_version: u32,
    tool: String,
    input: OutputIndexInput<'a>,
    root_package: Option<&'a str>,
    root_package_source: &'static str,
    counts: OutputIndexCounts,
    entries: &'a [LayoutEntry],
}

#[derive(Serialize)]
struct OutputIndexInput<'a> {
    snapshot_hash: &'a str,
    target_arch: &'a str,
}

#[derive(Serialize)]
struct OutputIndexCounts {
    functions: usize,
    source_files: usize,
    libraries: usize,
    unattributed: usize,
    path_collisions: usize,
}

fn package_from_library_uri(uri: &str) -> Option<&str> {
    let rest = uri.strip_prefix("package:")?;
    let (package, _) = rest.split_once('/').unwrap_or((rest, ""));
    if package.is_empty() {
        None
    } else {
        Some(package)
    }
}

fn select_root_package(
    explicit: &[String],
    hints: &ProgramHints,
    priority_package_hints: &[String],
    app_package_counts: &[(String, usize)],
) -> (Option<String>, &'static str) {
    if explicit.len() == 1 {
        let package = explicit[0].trim();
        if !package.is_empty() {
            return (Some(package.to_string()), "flag");
        }
    }
    for hint in hints.iter() {
        if matches!(hint.kind, HintKind::EntryPoint | HintKind::BootMain) {
            if let Some(package) = hint.library_uri.as_deref().and_then(package_from_library_uri)
            {
                return (Some(package.to_string()), "entrypoint");
            }
        }
    }
    let named: HashSet<&str> = priority_package_hints.iter().map(String::as_str).collect();
    for (package, _) in app_package_counts {
        if named.contains(package.as_str()) {
            return (Some(package.clone()), "app_package_counts");
        }
    }
    (None, "none")
}

fn is_windows_reserved(name: &str) -> bool {
    let base = name
        .split('.')
        .next()
        .unwrap_or(name)
        .to_ascii_uppercase();
    if matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$") {
        return true;
    }
    if base.len() == 4 && base.is_ascii() {
        let prefix = &base[..3];
        let digit = base.as_bytes()[3];
        return (prefix == "COM" || prefix == "LPT") && (b'1'..=b'9').contains(&digit);
    }
    false
}

/// Sanitize one path component. `suffix` is appended only when the raw name
/// had to be truncated; it is `__<function_id>` for a file and `__<hash8>` for a
/// directory.
fn sanitize_component(raw: &str, suffix: &str) -> (String, bool) {
    let max_base = MAX_COMPONENT_BYTES.saturating_sub(suffix.len());
    let mut out = String::new();
    let mut truncated = false;
    for c in raw.chars() {
        if out.len() >= max_base {
            truncated = true;
            break;
        }
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("unnamed");
    }
    if is_windows_reserved(&out) {
        out.insert(0, '_');
    }
    if truncated {
        out.push_str(suffix);
    }
    if out.len() > MAX_COMPONENT_BYTES {
        out.truncate(MAX_COMPONENT_BYTES);
    }
    (out, truncated)
}

/// Like [`sanitize_component`], but keeps `.` because a file stem joins the
/// class and function with it. Path navigation is still impossible: a stem of
/// `.` or `..` becomes `_`.
fn sanitize_file_stem(raw: &str, suffix: &str) -> (String, bool) {
    if raw == "." || raw == ".." {
        return ("_".to_string(), false);
    }
    let max_base = MAX_COMPONENT_BYTES.saturating_sub(suffix.len());
    let mut out = String::new();
    let mut truncated = false;
    for c in raw.chars() {
        if out.len() >= max_base {
            truncated = true;
            break;
        }
        if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("unnamed");
    }
    if is_windows_reserved(&out) {
        out.insert(0, '_');
    }
    if truncated {
        out.push_str(suffix);
    }
    if out.len() > MAX_COMPONENT_BYTES {
        out.truncate(MAX_COMPONENT_BYTES);
    }
    (out, truncated)
}

/// A short, deterministic digest used only to disambiguate path components.
/// It is not a content digest and is not presented as one.
fn hash8(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:08x}", (hash ^ (hash >> 32)) as u32)
}

fn sanitize_dir(raw: &str) -> String {
    sanitize_component(raw, &format!("__{}", hash8(raw))).0
}

fn sanitize_path(path: &str) -> String {
    let segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(sanitize_dir)
        .collect::<Vec<_>>();
    if segments.is_empty() {
        String::new()
    } else {
        format!("{}/", segments.join("/"))
    }
}

fn strip_at_id(name: &str) -> &str {
    if let Some(position) = name.rfind('@') {
        let suffix = &name[position + 1..];
        if !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit()) {
            return &name[..position];
        }
    }
    name
}

fn display_class(raw: &str) -> String {
    let stripped = strip_at_id(raw);
    if stripped.is_empty() {
        "unnamed".to_string()
    } else {
        stripped.to_string()
    }
}

/// Turn a raw model name into the name used in the path.
///
/// The model name is the authority, but serwalker qualifies constructors with
/// their owner (`Class@id._@id`). This strips that qualifier, strips the
/// compiler's trailing `@id`, and gives constructors and dynamic invocation
/// stubs a readable label. It never changes the exact name kept in the index.
fn display_function(raw: &str, class_raw: Option<&str>) -> Option<String> {
    let mut bare = raw;
    let mut qualified = false;
    if let Some(class) = class_raw {
        let class_display = strip_at_id(class);
        if let Some(rest) = raw.strip_prefix(class) {
            if let Some(rest) = rest.strip_prefix('.') {
                bare = rest;
                qualified = true;
            }
        }
        if !qualified {
            if let Some(rest) = raw.strip_prefix(class_display) {
                if let Some(rest) = rest.strip_prefix('.') {
                    bare = rest;
                    qualified = true;
                }
            }
        }
    }
    let bare = strip_at_id(bare);
    if bare == "dyn:" || bare == "dyn" {
        return Some("dyn_invoke".to_string());
    }
    if let Some(class) = class_raw {
        let class_display = strip_at_id(class);
        let constructor_like = bare == class_display
            || (bare.len() == class_display.len() + 1
                && bare.starts_with(class_display)
                && bare.ends_with('_'))
            || (qualified && (bare == "_" || bare.is_empty()));
        if constructor_like {
            return Some("new".to_string());
        }
    }
    if bare.is_empty() {
        None
    } else {
        Some(bare.to_string())
    }
}

fn stem_for(function: &LayoutFunction) -> String {
    let class = function.class.as_deref().map(display_class);
    let raw_name = function
        .model_name
        .as_deref()
        .or(Some(function.emitted_name.as_str()));
    let function_name = raw_name.and_then(|name| display_function(name, function.class.as_deref()));
    match (class, function_name) {
        (Some(class), Some(name)) => format!("{class}.{name}"),
        (Some(class), None) => format!("{class}.fn_0x{:x}", function.entry_va),
        (None, Some(name)) => name,
        (None, None) => format!("fn_0x{:x}", function.entry_va),
    }
}

fn file_uri_directory(uri: &str) -> String {
    let raw = uri
        .strip_prefix("file://")
        .or_else(|| uri.strip_prefix("file:"))
        .unwrap_or(uri)
        .trim_start_matches('/');
    if let Some(position) = raw.find(".dart_tool/flutter_build/") {
        let suffix = &raw[position + ".dart_tool/flutter_build/".len()..];
        let suffix = suffix.strip_suffix(".dart").unwrap_or(suffix);
        let mut segments = suffix
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        if segments.len() >= 2 {
            let target = segments.remove(0);
            let rest = segments.join("/");
            return format!("build/{}/{}", sanitize_dir(target), sanitize_path(&rest));
        }
        if segments.len() == 1 {
            return format!("build/{}/", sanitize_dir(segments[0]));
        }
    }
    let base = raw.rsplit('/').next().unwrap_or("file");
    let base = base.strip_suffix(".dart").unwrap_or(base);
    format!("build/external/{}/{}/", hash8(uri), sanitize_dir(base))
}

fn directory_for_uri(uri: Option<&str>, root_package: Option<&str>) -> (String, Placement) {
    let Some(uri) = uri else {
        return ("unattributed/".to_string(), Placement::None);
    };
    if let Some(rest) = uri.strip_prefix("package:") {
        let (package, path) = rest.split_once('/').unwrap_or((rest, ""));
        let package_dir = sanitize_dir(package);
        let path = path.strip_suffix(".dart").unwrap_or(path);
        let suffix = sanitize_path(path);
        if root_package == Some(package) {
            return (format!("lib/{suffix}"), Placement::Model);
        }
        return (
            format!("packages/{package_dir}/lib/{suffix}"),
            Placement::Model,
        );
    }
    if let Some(library) = uri.strip_prefix("dart:") {
        return (format!("sdk/{}/", sanitize_dir(library)), Placement::Model);
    }
    if uri.starts_with("file:") {
        return (file_uri_directory(uri), Placement::Heuristic);
    }
    ("unattributed/".to_string(), Placement::None)
}

fn append_dir_suffix(directory: &str, suffix: &str) -> String {
    let trimmed = directory.trim_end_matches('/');
    match trimmed.rsplit_once('/') {
        Some((parent, last)) => format!("{parent}/{last}{suffix}/"),
        None => format!("{trimmed}{suffix}/"),
    }
}

fn append_file_suffix(path: &str, suffix: &str) -> String {
    match path.strip_suffix(".dart") {
        Some(stem) => format!("{stem}{suffix}.dart"),
        None => format!("{path}{suffix}"),
    }
}

fn resolve_directories(
    functions: &[LayoutFunction],
    root_package: Option<&str>,
) -> (Vec<String>, Vec<bool>) {
    let mut unique = BTreeSet::new();
    for function in functions {
        if let Some(library) = &function.library {
            unique.insert(library.clone());
        }
    }
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for library in &unique {
        let (directory, _) = directory_for_uri(Some(library), root_package);
        groups
            .entry(directory.to_ascii_lowercase())
            .or_default()
            .push(library.clone());
    }
    let mut resolved: BTreeMap<String, (String, bool)> = BTreeMap::new();
    for (_, mut group) in groups {
        group.sort();
        for (index, library) in group.iter().enumerate() {
            let (mut directory, _) = directory_for_uri(Some(library), root_package);
            let mut collision = false;
            if index > 0 {
                directory = append_dir_suffix(&directory, &format!("__{}", hash8(library)));
                collision = true;
            }
            resolved.insert(library.clone(), (directory, collision));
        }
    }
    let mut directories = Vec::with_capacity(functions.len());
    let mut collisions = Vec::with_capacity(functions.len());
    for function in functions {
        match &function.library {
            Some(library) => {
                let (directory, collision) = resolved
                    .get(library)
                    .cloned()
                    .unwrap_or_else(|| ("unattributed/".to_string(), false));
                directories.push(directory);
                collisions.push(collision);
            }
            None => {
                directories.push("unattributed/".to_string());
                collisions.push(false);
            }
        }
    }
    (directories, collisions)
}

fn plan_layout(
    functions: &[LayoutFunction],
    root_package: Option<&str>,
    root_package_source: &'static str,
) -> LayoutPlan {
    let (directories, directory_collisions) = resolve_directories(functions, root_package);
    struct Candidate {
        function_index: usize,
        path: String,
        collision: bool,
    }
    let mut candidates = Vec::with_capacity(functions.len());
    for (index, function) in functions.iter().enumerate() {
        let stem = stem_for(function);
        let (stem, truncated) =
            sanitize_file_stem(&stem, &format!("__{}", function.function_id));
        let mut path = format!("{}{stem}.dart", directories[index]);
        let mut collision = directory_collisions[index] || truncated;
        if path.len() > MAX_RELATIVE_PATH_BYTES {
            path = format!("unattributed/fn_0x{:x}.dart", function.entry_va);
            collision = true;
        }
        candidates.push(Candidate {
            function_index: index,
            path,
            collision,
        });
    }

    let mut ordered = (0..candidates.len()).collect::<Vec<_>>();
    ordered.sort_by_key(|candidate_index| {
        (
            functions[candidates[*candidate_index].function_index].function_id,
            *candidate_index,
        )
    });
    let mut used_paths = HashSet::new();
    for candidate_index in ordered {
        let candidate = &mut candidates[candidate_index];
        let mut path = candidate.path.clone();
        let function_id = functions[candidate.function_index].function_id;
        if used_paths.contains(&path.to_ascii_lowercase()) {
            let base = path.clone();
            let mut suffix = format!("__{function_id}");
            path = append_file_suffix(&base, &suffix);
            let mut attempt = 2;
            while used_paths.contains(&path.to_ascii_lowercase()) {
                suffix = format!("__{function_id}_{attempt}");
                path = append_file_suffix(&base, &suffix);
                attempt += 1;
            }
            candidate.collision = true;
        }
        used_paths.insert(path.to_ascii_lowercase());
        candidate.path = path;
    }

    let mut entries = candidates
        .iter()
        .map(|candidate| {
            let function = &functions[candidate.function_index];
            let placement = match function.placement {
                Placement::None if function.library.is_some() => Placement::Model,
                placement => placement,
            };
            let placement = if function
                .library
                .as_deref()
                .is_some_and(|library| library.starts_with("file:"))
            {
                Placement::Heuristic
            } else {
                placement
            };
            LayoutEntry {
                function_id: function.function_id,
                path: candidate.path.clone(),
                name: function.emitted_name.clone(),
                model_name: function.model_name.clone(),
                class: function.class.clone(),
                library: function.library.clone(),
                entry_va: format!("0x{:x}", function.entry_va),
                function_provenance: function.function_provenance.map(Provenance::as_str),
                class_provenance: function.class_provenance.map(Provenance::as_str),
                library_provenance: function.library_provenance.map(Provenance::as_str),
                placement,
                collision: candidate.collision,
                detail_path: candidate.path.clone(),
                line_start: None,
                line_end: None,
                declared_name: None,
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| {
        a.function_id
            .cmp(&b.function_id)
            .then_with(|| a.path.cmp(&b.path))
    });
    let collisions = entries.iter().filter(|entry| entry.collision).count();
    let unattributed = entries
        .iter()
        .filter(|entry| entry.placement == Placement::None)
        .count();
    LayoutPlan {
        root_package: root_package.map(str::to_string),
        root_package_source,
        entries,
        collisions,
        unattributed,
    }
}

fn build_layout_functions(
    model: &ProgramModel,
    pseudo: &[PseudocodeArtifact],
    disasm: &[FunctionDisassembly],
    hints: &ProgramHints,
    root_package: Option<&str>,
) -> Vec<LayoutFunction> {
    let functions_by_id: HashMap<u64, &Function> = model
        .functions
        .iter()
        .map(|function| (u64::from(function.id.0), function))
        .collect();
    let entries_by_id: HashMap<u64, u64> = disasm
        .iter()
        .map(|function| (function.function_id, function.entry_va))
        .collect();
    let main_hints: HashMap<u64, String> = hints
        .iter()
        .filter(|hint| matches!(hint.kind, HintKind::EntryPoint | HintKind::BootMain))
        .filter_map(|hint| Some((hint.target_va?, hint.library_uri.clone()?)))
        .collect();
    let root_main_uri = root_package.and_then(|package| {
        let uri = format!("package:{package}/main.dart");
        model
            .libraries
            .iter()
            .find(|library| library.uri == uri)
            .map(|library| library.uri.clone())
    });

    let entry_va_for = |function_id: u64| {
        functions_by_id
            .get(&function_id)
            .map(|function| function.code.start_va)
            .or_else(|| entries_by_id.get(&function_id).copied())
            .unwrap_or(0)
    };
    // The model has no function-to-library edge. The one safe fallback is the
    // lowest-address named `main` in the whole model, which is the entry point on
    // the samples seen so far. A filtered subset that contains only another
    // `main` does not get promoted: that would be an invented owner.
    let root_main_entry = root_main_uri.as_ref().and_then(|_| {
        model
            .functions
            .iter()
            .filter(|function| {
                function.owner.is_none() && function.name_text() == Some("main")
            })
            .map(|function| function.code.start_va)
            .min()
    });

    pseudo
        .iter()
        .map(|artifact| {
            let function = functions_by_id.get(&artifact.function_id).copied();
            let entry_va = entry_va_for(artifact.function_id);
            let class = function
                .and_then(|function| model.owner_name(function))
                .map(str::to_string);
            let mut library = function
                .and_then(|function| model.owner_library_uri(function))
                .map(str::to_string);
            let class_provenance = function
                .and_then(|function| function.owner)
                .and_then(|id| model.class(id))
                .map(|class| class.provenance);
            let mut library_provenance = function
                .and_then(|function| function.owner)
                .and_then(|id| model.class(id))
                .and_then(|class| class.library)
                .and_then(|id| model.library(id))
                .map(|library| library.provenance);
            let model_name = function
                .and_then(|function| function.name_text())
                .map(str::to_string);
            let mut placement = if library.is_some() {
                Placement::Model
            } else {
                Placement::None
            };
            if class.is_none() && library.is_none() && model_name.as_deref() == Some("main") {
                if let Some(hint_library) = main_hints.get(&entry_va) {
                    library = Some(hint_library.clone());
                    placement = Placement::Heuristic;
                } else if root_main_entry == Some(entry_va) {
                    if let Some(root_main) = &root_main_uri {
                        library = Some(root_main.clone());
                        placement = Placement::Heuristic;
                    }
                }
            }
            if library_provenance.is_none() {
                library_provenance = library.as_deref().and_then(|uri| {
                    model
                        .libraries
                        .iter()
                        .find(|library| library.uri == uri)
                        .map(|library| library.provenance)
                });
            }
            let emitted_name = display_function(&artifact.function_name, class.as_deref())
                .unwrap_or_else(|| format!("fn_0x{entry_va:x}"));
            LayoutFunction {
                function_id: artifact.function_id,
                entry_va,
                emitted_name,
                model_name,
                class,
                class_provenance,
                library,
                library_provenance,
                function_provenance: function.map(|function| function.provenance),
                placement,
            }
        })
        .collect()
}

fn pubspec_name(root_package: Option<&str>) -> String {
    let raw = root_package.unwrap_or("recovered_app");
    let mut out = String::new();
    for c in raw.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("recovered_app");
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert_str(0, "app_");
    }
    out
}

fn project_readme(root_package: Option<&str>, layout: OutputLayout) -> String {
    let root_line = match root_package {
        Some(package) => format!("- `lib/` is the root package, `{package}`."),
        None => "- `lib/` is not present because no root package was selected.".to_string(),
    };
    let shape = match layout {
        OutputLayout::Library => {
            "Each `.dart` file holds the functions recovered for one library, except \
             `unattributed/`, which keeps one function per file. Declarations are \
             owner-qualified top-level functions; the class membership is recorded \
             in `index.json`, not asserted as a class body.\n"
        }
        OutputLayout::Project => {
            "Each `.dart` file holds one recovered function, named for its class \
             and function when the model knows them.\n"
        }
    };
    format!(
        "# Recovered Flutter project tree\n\n\
         This tree was written by `flutterdec decompile` from a compiled Flutter\n\
         snapshot. The files are recovered pseudo-Dart, not original source. They\n\
         are not expected to pass `dart analyze`: types, imports and the Dart\n\
         runtime are not reconstructed.\n\n\
         {root_line}\n\
         - `packages/` holds dependency packages.\n\
         - `sdk/` holds Dart SDK libraries.\n\
         - `build/` holds libraries from Flutter build paths.\n\
         - `unattributed/` holds functions the model could not place.\n\
         - `artifacts/` holds optional assembly, IR, and provenance source artifacts.\n\n\
         {shape}\n\
         Use `index.json` to map every function to its source file, function id,\n\
         library, class, entry address and provenance. Composed functions also\n\
         carry `line_start` and `line_end`. `name` is the display name;\n\
         `model_name` keeps the exact recovered name.\n"
    )
}

fn safe_join(root: &Path, relative: &str) -> Result<PathBuf> {
    let mut path = root.to_path_buf();
    for segment in relative.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            bail!("output path escapes the output root: {relative}");
        }
        path.push(segment);
    }
    Ok(path)
}

fn write_relative(root: &Path, relative: &str, bytes: &[u8]) -> Result<()> {
    let path = safe_join(root, relative)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    Ok(())
}

#[derive(serde::Deserialize)]
struct PreviousIndex {
    entries: Vec<PreviousIndexEntry>,
}

#[derive(serde::Deserialize)]
struct PreviousIndexEntry {
    path: String,
}

fn cleanup_stale_files(out_dir: &Path, new_paths: &HashSet<String>) -> Result<()> {
    let index_path = out_dir.join("index.json");
    let Ok(bytes) = fs::read(&index_path) else {
        return Ok(());
    };
    let Ok(previous) = serde_json::from_slice::<PreviousIndex>(&bytes) else {
        return Ok(());
    };
    for entry in previous.entries {
        if new_paths.contains(&entry.path) || !entry.path.ends_with(".dart") {
            continue;
        }
        let Ok(path) = safe_join(out_dir, &entry.path) else {
            continue;
        };
        if path.is_file() {
            fs::remove_file(&path)?;
            let mut parent = path.parent();
            while let Some(directory) = parent {
                if directory == out_dir {
                    break;
                }
                if fs::remove_dir(directory).is_err() {
                    break;
                }
                parent = directory.parent();
            }
        }
    }
    Ok(())
}

fn artifact_path(kind: &str, source_path: &str, extension: &str) -> String {
    let stem = source_path.strip_suffix(".dart").unwrap_or(source_path);
    format!("artifacts/{kind}/{stem}.{extension}")
}

/// Remove the artifact trees this writer owns before a run. The source tree may
/// keep a file that a previous run also produced, so stale asm/IR/audit-source
/// files cannot be found by comparing source paths alone. Clearing the trees
/// keeps layout switches and emit-flag changes deterministic.
fn reset_artifact_trees(out_dir: &Path) -> Result<()> {
    for kind in ["asm", "ir", "functions"] {
        let path = out_dir.join("artifacts").join(kind);
        if path.is_dir() {
            fs::remove_dir_all(&path)?;
        }
    }
    let artifacts = out_dir.join("artifacts");
    if artifacts.is_dir() && fs::read_dir(&artifacts)?.next().is_none() {
        fs::remove_dir(&artifacts)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct WriteOptions<'a> {
    layout: OutputLayout,
    emit_asm: bool,
    emit_asm_opcodes: bool,
    emit_ir: bool,
    snapshot_hash: &'a str,
    target_arch: &'a str,
}

fn write_project_layout(
    out_dir: &Path,
    plan: &LayoutPlan,
    pseudo: &[PseudocodeArtifact],
    disasm: &[FunctionDisassembly],
    ir: &[FunctionIr],
    options: WriteOptions<'_>,
) -> Result<OutputLayoutSummary> {
    if options.layout == OutputLayout::Library {
        return write_library_layout(out_dir, plan, pseudo, disasm, ir, options);
    }
    let WriteOptions {
        layout: _,
        emit_asm,
        emit_asm_opcodes,
        emit_ir,
        snapshot_hash,
        target_arch,
    } = options;
    fs::create_dir_all(out_dir)?;
    reset_artifact_trees(out_dir)?;
    if out_dir.join("pseudocode").is_dir() && !out_dir.join("index.json").is_file() {
        eprintln!(
            "warning: {} contains a pre-layout `pseudocode/` tree; the new files are written \
             alongside it. Remove that directory if you no longer need the old flat output.",
            out_dir.display()
        );
    }
    let pseudo_by_id: HashMap<u64, &PseudocodeArtifact> = pseudo
        .iter()
        .map(|artifact| (artifact.function_id, artifact))
        .collect();
    let disasm_by_id: HashMap<u64, &FunctionDisassembly> = disasm
        .iter()
        .map(|function| (function.function_id, function))
        .collect();
    let ir_by_id: HashMap<u64, &FunctionIr> =
        ir.iter().map(|function| (function.function_id, function)).collect();

    let new_paths: HashSet<String> = plan
        .entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect();
    // Remove the previous run's sources before writing this run's, so a
    // case-only path change cannot delete the file that was just written.
    cleanup_stale_files(out_dir, &new_paths)?;
    for entry in &plan.entries {
        let artifact = pseudo_by_id.get(&entry.function_id).with_context(|| {
            format!(
                "layout entry {} has no pseudocode artifact",
                entry.function_id
            )
        })?;
        write_relative(out_dir, &entry.path, terminated(&artifact.source).as_bytes())?;

        if emit_asm {
            if let Some(function) = disasm_by_id.get(&entry.function_id) {
                let mut lines = Vec::with_capacity(function.instructions.len());
                for instruction in &function.instructions {
                    lines.push(format_asm_instruction_line(instruction, emit_asm_opcodes));
                }
                let path = artifact_path("asm", &entry.path, "s");
                write_relative(out_dir, &path, terminated(&lines.join("\n")).as_bytes())?;
            }
        }
        if emit_ir {
            if let Some(function) = ir_by_id.get(&entry.function_id) {
                let mut body = serde_json::to_vec_pretty(function)?;
                body.push(b'\n');
                let path = artifact_path("ir", &entry.path, "json");
                write_relative(out_dir, &path, &body)?;
            }
        }
    }

    let pubspec = format!(
        "name: {}\ndescription: Recovered pseudocode written by flutterdec. Not original source.\npublish_to: none\n",
        pubspec_name(plan.root_package.as_deref())
    );
    write_relative(out_dir, "pubspec.yaml", pubspec.as_bytes())?;
    write_relative(
        out_dir,
        "README.md",
        project_readme(plan.root_package.as_deref(), OutputLayout::Project).as_bytes(),
    )?;

    let index = OutputIndex {
        schema_version: 1,
        layout: "project",
        layout_version: LAYOUT_VERSION,
        tool: format!("flutterdec {}", env!("CARGO_PKG_VERSION")),
        input: OutputIndexInput {
            snapshot_hash,
            target_arch,
        },
        root_package: plan.root_package.as_deref(),
        root_package_source: plan.root_package_source,
        counts: OutputIndexCounts {
            functions: plan.entries.len(),
            source_files: plan.entries.len(),
            libraries: 0,
            unattributed: plan.unattributed,
            path_collisions: plan.collisions,
        },
        entries: &plan.entries,
    };
    let mut index_bytes = serde_json::to_vec_pretty(&index)?;
    index_bytes.push(b'\n');
    write_relative(out_dir, "index.json", &index_bytes)?;

    Ok(OutputLayoutSummary {
        layout: "project",
        root_package: plan.root_package.clone(),
        root_package_source: plan.root_package_source,
        source_files: plan.entries.len(),
        libraries: 0,
        unattributed: plan.unattributed,
        path_collisions: plan.collisions,
    })
}

#[derive(Debug, Clone)]
struct LibraryGroup {
    uri: String,
    directory: String,
    path: String,
    collision: bool,
    entries: Vec<usize>,
}

fn append_library_suffix(path: &str, suffix: &str) -> String {
    let stem = path.strip_suffix(".dart").unwrap_or(path);
    format!("{stem}__{suffix}.dart")
}

fn composed_declaration_name(entry: &LayoutEntry, used: &mut HashSet<String>) -> String {
    let class = entry.class.as_deref().map(display_class);
    let raw = match class {
        Some(class) => format!("{class}_{}", entry.name),
        None => entry.name.clone(),
    };
    let mut name = sanitize_identifier(&raw);
    if used.contains(&name) {
        name = sanitize_identifier(&format!(
            "{}_{}",
            name.trim_end_matches('_'),
            entry.function_id
        ));
    }
    while used.contains(&name) {
        name.push('_');
    }
    used.insert(name.clone());
    name
}

fn sanitize_identifier(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("fn");
    }
    if out.as_bytes()[0].is_ascii_digit() {
        out.insert(0, '_');
    }
    if is_dart_keyword(&out) {
        out.insert(0, '_');
    }
    out
}

fn is_dart_keyword(name: &str) -> bool {
    matches!(
        name,
        "abstract"
            | "as"
            | "assert"
            | "async"
            | "await"
            | "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "covariant"
            | "default"
            | "deferred"
            | "do"
            | "dynamic"
            | "else"
            | "enum"
            | "export"
            | "extends"
            | "extension"
            | "external"
            | "factory"
            | "false"
            | "final"
            | "finally"
            | "for"
            | "Function"
            | "get"
            | "hide"
            | "if"
            | "implements"
            | "import"
            | "in"
            | "interface"
            | "is"
            | "late"
            | "library"
            | "mixin"
            | "new"
            | "null"
            | "on"
            | "operator"
            | "part"
            | "required"
            | "rethrow"
            | "return"
            | "set"
            | "show"
            | "static"
            | "super"
            | "switch"
            | "sync"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "typedef"
            | "var"
            | "void"
            | "while"
            | "with"
            | "yield"
    )
}

fn rename_function_declaration(source: &str, old: &str, new: &str) -> String {
    if old == new {
        return source.to_string();
    }
    let marker = format!("dynamic {old}(");
    let Some(position) = source.find(&marker) else {
        return source.to_string();
    };
    let mut out = String::with_capacity(source.len() + new.len());
    out.push_str(&source[..position]);
    out.push_str(&format!("dynamic {new}("));
    out.push_str(&source[position + marker.len()..]);
    out
}

/// Rename `_block_<n>` helpers so several functions can share one file. The
/// scanner skips comments and strings; a body's block helper names are an
/// emitter detail, not a claim about the source program.
fn rename_block_helpers(source: &str, function_id: u64) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            let start = i;
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            out.push_str(&source[start..i]);
            continue;
        }
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            out.push_str(&source[start..i]);
            continue;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                } else if bytes[i] == quote {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            out.push_str(&source[start..i]);
            continue;
        }
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let identifier = &source[start..i];
            if let Some(number) = identifier.strip_prefix("_block_") {
                if !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()) {
                    out.push_str(&format!("_block_{function_id}_{number}"));
                    continue;
                }
            }
            out.push_str(identifier);
            continue;
        }
        let ch = source[i..].chars().next().expect("in bounds");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn write_library_layout(
    out_dir: &Path,
    plan: &LayoutPlan,
    pseudo: &[PseudocodeArtifact],
    disasm: &[FunctionDisassembly],
    ir: &[FunctionIr],
    options: WriteOptions<'_>,
) -> Result<OutputLayoutSummary> {
    let WriteOptions {
        layout: _,
        emit_asm,
        emit_asm_opcodes,
        emit_ir,
        snapshot_hash,
        target_arch,
    } = options;
    fs::create_dir_all(out_dir)?;
    reset_artifact_trees(out_dir)?;
    if out_dir.join("pseudocode").is_dir() && !out_dir.join("index.json").is_file() {
        eprintln!(
            "warning: {} contains a pre-layout `pseudocode/` tree; the new files are written \
             alongside it. Remove that directory if you no longer need the old flat output.",
            out_dir.display()
        );
    }
    let pseudo_by_id: HashMap<u64, &PseudocodeArtifact> = pseudo
        .iter()
        .map(|artifact| (artifact.function_id, artifact))
        .collect();
    let disasm_by_id: HashMap<u64, &FunctionDisassembly> = disasm
        .iter()
        .map(|function| (function.function_id, function))
        .collect();
    let ir_by_id: HashMap<u64, &FunctionIr> =
        ir.iter().map(|function| (function.function_id, function)).collect();

    let mut entries = plan.entries.clone();
    let mut groups_by_uri: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, entry) in entries.iter().enumerate() {
        if entry.library.is_some() && !entry.path.starts_with("unattributed/") {
            groups_by_uri
                .entry(entry.library.clone().expect("checked above"))
                .or_default()
                .push(index);
        }
    }
    let mut groups = groups_by_uri
        .into_iter()
        .filter_map(|(uri, entries_for_group)| {
            let directory = entries
                .get(entries_for_group[0])
                .and_then(|entry| entry.path.rsplit_once('/').map(|(directory, _)| directory))?;
            if directory.is_empty() {
                return None;
            }
            Some(LibraryGroup {
                uri,
                directory: directory.to_string(),
                path: String::new(),
                collision: false,
                entries: entries_for_group,
            })
        })
        .collect::<Vec<_>>();
    let composed_indices: HashSet<usize> = groups
        .iter()
        .flat_map(|group| group.entries.iter().copied())
        .collect();
    let mut used_paths: HashSet<String> = entries
        .iter()
        .enumerate()
        .filter(|(index, _)| !composed_indices.contains(index))
        .map(|(_, entry)| entry.path.to_ascii_lowercase())
        .collect();
    for group in &mut groups {
        let mut candidate = format!("{}.dart", group.directory);
        if candidate.len() > MAX_RELATIVE_PATH_BYTES {
            candidate = append_library_suffix(&candidate, &hash8(&group.uri));
            group.collision = true;
        }
        while used_paths.contains(&candidate.to_ascii_lowercase()) {
            candidate = append_library_suffix(&candidate, &hash8(&group.uri));
            group.collision = true;
        }
        used_paths.insert(candidate.to_ascii_lowercase());
        group.path = candidate;
    }

    let new_paths: HashSet<String> = groups
        .iter()
        .map(|group| group.path.clone())
        .chain(
            entries
                .iter()
                .enumerate()
                .filter(|(index, _)| !composed_indices.contains(index))
                .map(|(_, entry)| entry.path.clone()),
        )
        .collect();
    // Clean sources before composing, not after. A case-only rename resolves to
    // the same file on a case-insensitive filesystem, so writing first and
    // removing the old spelling would delete the new library.
    cleanup_stale_files(out_dir, &new_paths)?;

    // The provenance audit records coordinates against each function's own
    // source, before composition changes line numbers or helper names. Keep
    // those exact sources under artifacts/functions when an audit is requested
    // so the coordinates remain resolvable in library layout.
    if std::env::var_os("FLUTTERDEC_PROV_AUDIT").is_some() {
        for entry in &entries {
            let artifact = pseudo_by_id.get(&entry.function_id).with_context(|| {
                format!(
                    "layout entry {} has no pseudocode artifact",
                    entry.function_id
                )
            })?;
            let path = format!("artifacts/functions/{}.dart", entry.function_id);
            write_relative(out_dir, &path, terminated(&artifact.source).as_bytes())?;
        }
    }

    for group in &groups {
        let mut composed = format!(
            "// library: {}\n\
             // Recovered pseudo-Dart by flutterdec. Not original source.\n\
             // Functions in this file: {}\n\n",
            group.uri,
            group.entries.len()
        );
        let mut used_names = HashSet::new();
        let mut ordered = group.entries.clone();
        ordered.sort_by(|a, b| {
            let a = &entries[*a];
            let b = &entries[*b];
            a.class
                .cmp(&b.class)
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.function_id.cmp(&b.function_id))
        });
        let mut next_line = composed.lines().count() + 1;
        for index in ordered {
            let entry = &mut entries[index];
            let artifact = pseudo_by_id.get(&entry.function_id).with_context(|| {
                format!(
                    "layout entry {} has no pseudocode artifact",
                    entry.function_id
                )
            })?;
            let declared = composed_declaration_name(entry, &mut used_names);
            let mut source = rename_block_helpers(&artifact.source, entry.function_id);
            source = rename_function_declaration(&source, &artifact.function_name, &declared);
            let source = source.trim_end_matches('\n');
            let header = format!(
                "// function_id: {} | class: {} | function: {} | entry_va: {}\n",
                entry.function_id,
                entry.class.as_deref().unwrap_or("-"),
                entry.model_name.as_deref().unwrap_or("-"),
                entry.entry_va
            );
            let start = next_line + header.lines().count();
            let body_lines = source.lines().count();
            composed.push_str(&header);
            composed.push_str(source);
            composed.push('\n');
            composed.push('\n');
            next_line = start + body_lines + 1;
            let end = start + body_lines.saturating_sub(1);
            entry.path = group.path.clone();
            entry.line_start = Some(start);
            entry.line_end = Some(end);
            entry.declared_name = Some(declared);
            entry.collision = group.collision;
        }
        write_relative(out_dir, &group.path, terminated(&composed).as_bytes())?;
    }

    for (index, entry) in entries.iter_mut().enumerate() {
        if composed_indices.contains(&index) {
            continue;
        }
        let artifact = pseudo_by_id.get(&entry.function_id).with_context(|| {
            format!(
                "layout entry {} has no pseudocode artifact",
                entry.function_id
            )
        })?;
        write_relative(out_dir, &entry.path, terminated(&artifact.source).as_bytes())?;
    }

    for entry in &entries {
        if emit_asm {
            if let Some(function) = disasm_by_id.get(&entry.function_id) {
                let mut lines = Vec::with_capacity(function.instructions.len());
                for instruction in &function.instructions {
                    lines.push(format_asm_instruction_line(instruction, emit_asm_opcodes));
                }
                let path = artifact_path("asm", &entry.detail_path, "s");
                write_relative(out_dir, &path, terminated(&lines.join("\n")).as_bytes())?;
            }
        }
        if emit_ir {
            if let Some(function) = ir_by_id.get(&entry.function_id) {
                let mut body = serde_json::to_vec_pretty(function)?;
                body.push(b'\n');
                let path = artifact_path("ir", &entry.detail_path, "json");
                write_relative(out_dir, &path, &body)?;
            }
        }
    }

    let pubspec = format!(
        "name: {}\ndescription: Recovered pseudocode written by flutterdec. Not original source.\npublish_to: none\n",
        pubspec_name(plan.root_package.as_deref())
    );
    write_relative(out_dir, "pubspec.yaml", pubspec.as_bytes())?;
    write_relative(
        out_dir,
        "README.md",
        project_readme(plan.root_package.as_deref(), OutputLayout::Library).as_bytes(),
    )?;

    let collisions = entries.iter().filter(|entry| entry.collision).count();
    let index = OutputIndex {
        schema_version: 2,
        layout: "library",
        layout_version: LAYOUT_VERSION,
        tool: format!("flutterdec {}", env!("CARGO_PKG_VERSION")),
        input: OutputIndexInput {
            snapshot_hash,
            target_arch,
        },
        root_package: plan.root_package.as_deref(),
        root_package_source: plan.root_package_source,
        counts: OutputIndexCounts {
            functions: entries.len(),
            source_files: new_paths.len(),
            libraries: groups.len(),
            unattributed: plan.unattributed,
            path_collisions: collisions,
        },
        entries: &entries,
    };
    let mut index_bytes = serde_json::to_vec_pretty(&index)?;
    index_bytes.push(b'\n');
    write_relative(out_dir, "index.json", &index_bytes)?;

    Ok(OutputLayoutSummary {
        layout: "library",
        root_package: plan.root_package.clone(),
        root_package_source: plan.root_package_source,
        source_files: new_paths.len(),
        libraries: groups.len(),
        unattributed: plan.unattributed,
        path_collisions: collisions,
    })
}


#[cfg(test)]
mod output_layout_tests {
    use super::*;

    fn layout_function(
        id: u64,
        entry_va: u64,
        name: &str,
        class: Option<&str>,
        library: Option<&str>,
    ) -> LayoutFunction {
        LayoutFunction {
            function_id: id,
            entry_va,
            emitted_name: name.to_string(),
            model_name: Some(name.to_string()),
            class: class.map(str::to_string),
            class_provenance: class.map(|_| Provenance::Derived),
            library: library.map(str::to_string),
            library_provenance: library.map(|_| Provenance::Derived),
            function_provenance: Some(Provenance::Derived),
            placement: if library.is_some() {
                Placement::Model
            } else {
                Placement::None
            },
        }
    }

    #[test]
    fn selects_root_package_from_flag_before_counts() {
        let (root, source) = select_root_package(
            &["localsend_app".to_string()],
            &ProgramHints::new(),
            &[],
            &[("markdown".to_string(), 100)],
        );
        assert_eq!(root.as_deref(), Some("localsend_app"));
        assert_eq!(source, "flag");
    }

    #[test]
    fn does_not_promote_the_top_count_when_it_is_not_named() {
        let (root, source) = select_root_package(
            &[],
            &ProgramHints::new(),
            &["localsend_app".to_string()],
            &[
                ("markdown".to_string(), 100),
                ("localsend_app".to_string(), 10),
            ],
        );
        assert_eq!(root.as_deref(), Some("localsend_app"));
        assert_eq!(source, "app_package_counts");
    }

    #[test]
    fn maps_package_dart_and_missing_libraries_to_roots() {
        assert_eq!(
            directory_for_uri(Some("package:localsend_app/main.dart"), Some("localsend_app")),
            ("lib/main/".to_string(), Placement::Model)
        );
        assert_eq!(
            directory_for_uri(
                Some("package:collection/src/iterable_zip.dart"),
                Some("localsend_app")
            ),
            (
                "packages/collection/lib/src/iterable_zip/".to_string(),
                Placement::Model
            )
        );
        assert_eq!(
            directory_for_uri(Some("dart:core"), None),
            ("sdk/core/".to_string(), Placement::Model)
        );
        assert_eq!(
            directory_for_uri(None, Some("localsend_app")),
            ("unattributed/".to_string(), Placement::None)
        );
        assert_eq!(
            file_uri_directory(
                "file:///tmp/app/.dart_tool/flutter_build/hash/dart_plugin_registrant.dart"
            ),
            "build/hash/dart_plugin_registrant/"
        );
    }

    #[test]
    fn rechecks_generated_collision_suffixes_against_allocated_paths() {
        let functions = vec![
            layout_function(1, 0x10, "foo", None, None),
            layout_function(2, 0x20, "foo", None, None),
            layout_function(3, 0x30, "foo__2", None, None),
        ];
        let plan = plan_layout(&functions, Some("app"), "flag");
        let paths = plan
            .entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                "unattributed/foo.dart",
                "unattributed/foo__2.dart",
                "unattributed/foo__2__3.dart",
            ]
        );
        assert_eq!(plan.collisions, 2);
    }

    #[test]
    fn sanitizes_components_and_windows_reserved_names() {
        assert_eq!(sanitize_component("a/b", ""), ("a_b".to_string(), false));
        assert_eq!(sanitize_component("CON", ""), ("_CON".to_string(), false));
        let long = "x".repeat(MAX_COMPONENT_BYTES + 10);
        let (sanitized, truncated) = sanitize_component(&long, "__42");
        assert!(truncated);
        assert_eq!(sanitized.len(), MAX_COMPONENT_BYTES);
        assert!(sanitized.ends_with("__42"));
    }

    #[test]
    fn names_constructors_dynamic_stubs_and_private_classes() {
        assert_eq!(
            display_function("TranslationsAr_", Some("TranslationsAr")),
            Some("new".to_string())
        );
        assert_eq!(
            display_function("TranslationsAr.", Some("TranslationsAr")),
            Some("new".to_string())
        );
        assert_eq!(
            display_function(
                "_TranslationsDialogsAr@1111005983._@1111005983",
                Some("_TranslationsDialogsAr@1111005983")
            ),
            Some("new".to_string())
        );
        assert_eq!(
            display_function("dyn:", Some("InputStream")),
            Some("dyn_invoke".to_string())
        );
        assert_eq!(
            display_function("build", Some("_LanguagePageState@1258286367")),
            Some("build".to_string())
        );
        assert_eq!(
            display_class("_LanguagePageState@1258286367"),
            "_LanguagePageState"
        );
    }

    #[test]
    fn resolves_same_stem_and_case_only_collisions() {
        let functions = vec![
            layout_function(2, 0x10, "build", Some("Foo"), Some("package:app/main.dart")),
            layout_function(1, 0x20, "build", Some("Foo"), Some("package:app/main.dart")),
            layout_function(3, 0x30, "build", Some("foo"), Some("package:app/main.dart")),
        ];
        let plan = plan_layout(&functions, Some("app"), "flag");
        let paths = plan
            .entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                "lib/main/Foo.build.dart",
                "lib/main/Foo.build__2.dart",
                "lib/main/foo.build__3.dart",
            ]
        );
        assert_eq!(plan.collisions, 2);
    }

    #[test]
    fn places_root_main_as_a_labelled_heuristic() {
        let mut function = layout_function(7, 0xd54750, "main", None, None);
        function.library = Some("package:app/main.dart".to_string());
        function.library_provenance = Some(Provenance::Exact);
        function.placement = Placement::Heuristic;
        let plan = plan_layout(&[function], Some("app"), "flag");
        assert_eq!(plan.entries[0].path, "lib/main/main.dart");
        assert_eq!(plan.entries[0].placement, Placement::Heuristic);
        assert_eq!(plan.unattributed, 0);
    }

    #[test]
    fn keeps_dependency_packages_out_of_lib() {
        let function = layout_function(
            1,
            0x10,
            "build",
            Some("Widget"),
            Some("package:markdown/src/widget.dart"),
        );
        let plan = plan_layout(&[function], Some("localsend_app"), "flag");
        assert_eq!(
            plan.entries[0].path,
            "packages/markdown/lib/src/widget/Widget.build.dart"
        );
    }

    #[test]
    fn index_entries_are_sorted_and_keep_exact_names() {
        let functions = vec![
            layout_function(
                9,
                0x20,
                "build",
                Some("_LanguagePageState@1258286367"),
                Some("package:app/pages/language_page.dart"),
            ),
            layout_function(3, 0x10, "build", Some("HomePage"), Some("package:app/pages/home.dart")),
        ];
        let plan = plan_layout(&functions, Some("app"), "flag");
        assert_eq!(
            plan.entries
                .iter()
                .map(|entry| entry.function_id)
                .collect::<Vec<_>>(),
            vec![3, 9]
        );
        assert_eq!(
            plan.entries[1].path,
            "lib/pages/language_page/_LanguagePageState.build.dart"
        );
        assert_eq!(
            plan.entries[1].class.as_deref(),
            Some("_LanguagePageState@1258286367")
        );
        assert_eq!(plan.entries[1].name, "build");
    }

    fn artifact(function_id: u64, name: &str, source: &str) -> PseudocodeArtifact {
        PseudocodeArtifact {
            function_id,
            function_name: name.to_string(),
            source: source.to_string(),
            placeholder_ifs: 0,
            unresolved_cf: 0,
            raw_register_calls: 0,
            total_calls: 0,
            indirect_calls: 0,
            semantic_direct_calls: 0,
            semantic_indirect_calls: 0,
            dispatch_selector_calls: 0,
            dispatch_table_calls: 0,
            repeated_blocks: 0,
            unlifted_instructions: 0,
            target_va_symbol_calls: 0,
        }
    }

    #[test]
    fn falls_back_when_the_directory_path_exceeds_the_cap() {
        let segment = "a".repeat(120);
        let library = format!("package:app/{}.dart", vec![segment; 35].join("/"));
        let function = layout_function(1, 0x10, "build", Some("Foo"), Some(&library));
        let plan = plan_layout(&[function], Some("app"), "flag");
        assert_eq!(plan.entries[0].path, "unattributed/fn_0x10.dart");
        assert!(plan.entries[0].collision);
        assert_eq!(plan.unattributed, 0);
    }

    #[test]
    fn writes_the_tree_index_and_removes_stale_files() {
        let directory = tempfile::tempdir().expect("temp dir");
        let out = directory.path();
        let functions = vec![
            layout_function(1, 0x10, "build", Some("HomePage"), Some("package:app/main.dart")),
            layout_function(
                2,
                0x20,
                "startSession",
                Some("SendNotifier"),
                Some("package:app/provider/network/send_provider.dart"),
            ),
            layout_function(3, 0x1234, "fn_0x1234", None, None),
        ];
        let plan = plan_layout(&functions, Some("app"), "flag");
        let pseudo = vec![
            artifact(1, "build", "dynamic build() {}\n"),
            artifact(2, "startSession", "dynamic startSession() {}\n"),
            artifact(3, "fn_0x1234", "dynamic fn_0x1234() {}\n"),
        ];
        let options = WriteOptions {
            layout: OutputLayout::Project,
            emit_asm: false,
            emit_asm_opcodes: false,
            emit_ir: false,
            snapshot_hash: "80a49c7111088100a233b2ae788e1f48",
            target_arch: "arm64",
        };
        let summary = write_project_layout(out, &plan, &pseudo, &[], &[], options)
            .expect("write layout");
        assert_eq!(summary.source_files, 3);
        assert_eq!(summary.unattributed, 1);
        assert!(out.join("lib/main/HomePage.build.dart").is_file());
        assert!(out.join("lib/provider/network/send_provider/SendNotifier.startSession.dart").is_file());
        assert!(out.join("unattributed/fn_0x1234.dart").is_file());
        let index: serde_json::Value =
            serde_json::from_slice(&fs::read(out.join("index.json")).unwrap()).unwrap();
        assert_eq!(index["counts"]["source_files"], 3);
        assert_eq!(index["counts"]["unattributed"], 1);
        assert_eq!(index["root_package"], "app");
        assert_eq!(index["entries"][0]["path"], "lib/main/HomePage.build.dart");
        let pubspec = fs::read_to_string(out.join("pubspec.yaml")).unwrap();
        assert!(pubspec.contains("name: app"));
        let readme = fs::read_to_string(out.join("README.md")).unwrap();
        assert!(readme.contains("not original source"));

        let plan = plan_layout(&functions[..1], Some("app"), "flag");
        let options = WriteOptions {
            layout: OutputLayout::Project,
            emit_asm: false,
            emit_asm_opcodes: false,
            emit_ir: false,
            snapshot_hash: "80a49c7111088100a233b2ae788e1f48",
            target_arch: "arm64",
        };
        write_project_layout(out, &plan, &pseudo[..1], &[], &[], options)
            .expect("rewrite layout");
        assert!(!out
            .join("lib/provider/network/send_provider/SendNotifier.startSession.dart")
            .exists());
        assert!(!out.join("unattributed/fn_0x1234.dart").exists());
        assert!(out.join("lib/main/HomePage.build.dart").is_file());
    }

    #[test]
    fn library_layout_keeps_artifacts_it_writes_after_a_layout_switch() {
        let directory = tempfile::tempdir().expect("temp dir");
        let out = directory.path();
        let functions = vec![layout_function(
            1,
            0x10,
            "build",
            Some("HomePage"),
            Some("package:app/main.dart"),
        )];
        let pseudo = vec![artifact(1, "build", "dynamic build() {}\n")];
        let project_plan = plan_layout(&functions, Some("app"), "flag");
        let project_options = WriteOptions {
            layout: OutputLayout::Project,
            emit_asm: false,
            emit_asm_opcodes: false,
            emit_ir: false,
            snapshot_hash: "80a49c7111088100a233b2ae788e1f48",
            target_arch: "arm64",
        };
        write_project_layout(out, &project_plan, &pseudo, &[], &[], project_options)
            .expect("write project layout");
        assert!(out.join("lib/main/HomePage.build.dart").is_file());

        let library_plan = plan_layout(&functions, Some("app"), "flag");
        let disasm = vec![FunctionDisassembly {
            function_id: 1,
            function_name: Some("build".to_string()),
            owner_class: Some("HomePage".to_string()),
            entry_va: 0x10,
            size: 0,
            instructions: Vec::new(),
        }];
        let library_options = WriteOptions {
            layout: OutputLayout::Library,
            emit_asm: true,
            emit_asm_opcodes: false,
            emit_ir: false,
            snapshot_hash: "80a49c7111088100a233b2ae788e1f48",
            target_arch: "arm64",
        };
        write_project_layout(
            out,
            &library_plan,
            &pseudo,
            &disasm,
            &[],
            library_options,
        )
        .expect("write library layout");
        assert!(!out.join("lib/main/HomePage.build.dart").exists());
        assert!(out.join("lib/main.dart").is_file());
        assert!(out.join("artifacts/asm/lib/main/HomePage.build.s").is_file());
    }

    #[test]
    fn composes_one_file_per_library_and_renames_helpers() {
        let directory = tempfile::tempdir().expect("temp dir");
        let out = directory.path();
        let functions = vec![
            layout_function(1, 0x10, "build", Some("HomePage"), Some("package:app/main.dart")),
            layout_function(
                2,
                0x20,
                "initState",
                Some("HomePage"),
                Some("package:app/main.dart"),
            ),
            layout_function(
                3,
                0x30,
                "build",
                Some("OtherPage"),
                Some("package:app/pages/other.dart"),
            ),
            layout_function(4, 0x40, "fn_0x40", None, None),
        ];
        let plan = plan_layout(&functions, Some("app"), "flag");
        let pseudo = vec![
            artifact(
                1,
                "build",
                "dynamic build() {\n  return _block_1();\n}\n\ndynamic _block_1() {\n  return null;\n}\n",
            ),
            artifact(
                2,
                "initState",
                "dynamic initState() {\n  return _block_1();\n}\n\ndynamic _block_1() {\n  return null;\n}\n",
            ),
            artifact(3, "build", "dynamic build() {}\n"),
            artifact(4, "fn_0x40", "dynamic fn_0x40() {}\n"),
        ];
        let options = WriteOptions {
            layout: OutputLayout::Library,
            emit_asm: false,
            emit_asm_opcodes: false,
            emit_ir: false,
            snapshot_hash: "80a49c7111088100a233b2ae788e1f48",
            target_arch: "arm64",
        };
        let summary = write_project_layout(out, &plan, &pseudo, &[], &[], options)
            .expect("write library layout");
        assert_eq!(summary.layout, "library");
        assert_eq!(summary.source_files, 3);
        assert_eq!(summary.libraries, 2);
        assert_eq!(summary.unattributed, 1);

        assert!(out.join("lib/main.dart").is_file());
        assert!(out.join("lib/pages/other.dart").is_file());
        assert!(out.join("unattributed/fn_0x40.dart").is_file());
        assert!(!out.join("lib/main/HomePage.build.dart").exists());
        let main = fs::read_to_string(out.join("lib/main.dart")).unwrap();
        assert!(main.contains("dynamic HomePage_build("));
        assert!(main.contains("dynamic HomePage_initState("));
        assert!(main.contains("_block_1_1"));
        assert!(!main.contains("dynamic build("));
        assert!(!main.contains("dynamic initState("));
        let other = fs::read_to_string(out.join("lib/pages/other.dart")).unwrap();
        assert!(other.contains("dynamic OtherPage_build("));

        let index: serde_json::Value =
            serde_json::from_slice(&fs::read(out.join("index.json")).unwrap()).unwrap();
        assert_eq!(index["schema_version"], 2);
        assert_eq!(index["layout"], "library");
        assert_eq!(index["counts"]["source_files"], 3);
        assert_eq!(index["counts"]["libraries"], 2);
        let entries = index["entries"].as_array().expect("entries");
        assert_eq!(entries[0]["path"], "lib/main.dart");
        assert_eq!(entries[0]["declared_name"], "HomePage_build");
        assert_eq!(entries[0]["line_start"], 6);
        assert_eq!(entries[0]["line_end"], 12);
        assert_eq!(entries[1]["path"], "lib/main.dart");
        assert_eq!(entries[1]["declared_name"], "HomePage_initState");
        assert_eq!(entries[1]["line_start"], 15);
        let main_lines: Vec<&str> = main.lines().collect();
        assert!(main_lines[5].starts_with("dynamic HomePage_build("));
        assert!(main_lines[11].trim() == "}");
        assert!(main_lines[14].starts_with("dynamic HomePage_initState("));
        assert_eq!(entries[2]["path"], "lib/pages/other.dart");
        assert_eq!(entries[2]["declared_name"], "OtherPage_build");
        assert_eq!(entries[2]["line_start"], 6);
        assert_eq!(entries[3]["path"], "unattributed/fn_0x40.dart");
        assert!(entries[3].get("declared_name").is_none());
    }

    #[test]
    fn helper_rename_skips_comments_and_strings_and_preserves_unicode() {
        let source = "// _block_1\nlet x = \"_block_1\";\ndynamic _block_1() {}\nlet y = _block_1(); // ü\n";
        let renamed = rename_block_helpers(source, 7);
        assert!(renamed.contains("// _block_1"));
        assert!(renamed.contains("\"_block_1\""));
        assert!(renamed.contains("dynamic _block_7_1()"));
        assert!(renamed.contains("let y = _block_7_1();"));
        assert!(renamed.contains("// ü"));
    }

}
