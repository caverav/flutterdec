fn classify_standard_selector(raw: &str) -> Option<String> {
    for candidate in selector_candidates(raw) {
        if let Some(tag) = match_selector_candidate(&candidate) {
            return Some(format!("{} [selector]", tag));
        }
    }
    None
}

fn match_selector_candidate(candidate: &str) -> Option<&'static str> {
    for table in [
        FLUTTER_SELECTORS,
        DART_ASYNC_SELECTORS,
        DART_CORE_SELECTORS,
        DART_IO_SELECTORS,
        DART_VM_RUNTIME_SELECTORS,
        DART_TYPED_DATA_SELECTORS,
    ] {
        if let Some(tag) = lookup_selector(candidate, table) {
            return Some(tag);
        }
    }
    None
}

fn lookup_selector<'a>(candidate: &str, table: &'a [(&str, &'a str)]) -> Option<&'a str> {
    for (needle, tag) in table {
        if candidate == *needle {
            return Some(*tag);
        }
    }
    None
}
