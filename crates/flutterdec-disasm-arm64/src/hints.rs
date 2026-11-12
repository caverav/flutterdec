//! Host-derived analysis hints, kept out of the adapter's authoritative domains.
//!
//! Core enrichment learns things the adapter never saw: the launcher activity in
//! an `AndroidManifest.xml`, the Dart entrypoints an APK's startup evidence
//! names, the selectors that look like boot-flow handlers. All of it is useful
//! for deciding what to disassemble first, and none of it is a fact about the
//! snapshot's `ObjectPool`.
//!
//! The previous design wrote these into `ProgramModel::object_pool` with
//! `index = object_pool.len()`, which put derived guesses in the same index
//! space as hardware pool slots and grew that space on every enrichment pass.
//! A hint lives here instead: it has its own record type, it always names where
//! it came from and how strongly, and it cannot collide with, overwrite, or be
//! mistaken for anything the adapter authored.

use std::collections::BTreeSet;

/// Which host artifact a hint was read out of.
///
/// Kept distinct from [`HintProvenance`] because where evidence came from and
/// how strong it is are different questions: a manifest is an exact document
/// that still only supports a guess about Dart code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HintOrigin {
    /// `AndroidManifest.xml` in the input APK.
    AndroidManifest,
    /// The APK's Android startup evidence (dex entrypoints, activity classes).
    ApkStartup,
    /// A pattern match over names the adapter already recovered.
    ModelNamePattern,
}

impl HintOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AndroidManifest => "android_manifest",
            Self::ApkStartup => "apk_startup",
            Self::ModelNamePattern => "model_name_pattern",
        }
    }
}

/// How well a hint is known.
///
/// There is no `Exact`. A hint is by construction something the host inferred
/// rather than read out of the snapshot, so promoting one to exact would be the
/// authority upgrade this module exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HintProvenance {
    /// Read verbatim from a host artifact that states it.
    Derived,
    /// A guess from pattern evidence.
    Heuristic,
}

impl HintProvenance {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Derived => "derived",
            Self::Heuristic => "heuristic",
        }
    }
}

/// What a hint claims about a selector or address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HintKind {
    /// Program entry, by any route.
    EntryPoint,
    /// Dart `main`.
    BootMain,
    /// `runApp`.
    BootRunApp,
    /// Engine or binding initialization.
    BootstrapInit,
    /// Intent, route, or deep-link handling.
    DeepLinkHandler,
    /// An Android activity lifecycle callback.
    ActivityHandler,
}

impl HintKind {
    pub const ALL: [HintKind; 6] = [
        HintKind::EntryPoint,
        HintKind::BootMain,
        HintKind::BootRunApp,
        HintKind::BootstrapInit,
        HintKind::DeepLinkHandler,
        HintKind::ActivityHandler,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::EntryPoint => "entry_point",
            Self::BootMain => "boot_main",
            Self::BootRunApp => "boot_run_app",
            Self::BootstrapInit => "bootstrap_init",
            Self::DeepLinkHandler => "deep_link_handler",
            Self::ActivityHandler => "activity_handler",
        }
    }

    /// The bootflow category name this hint contributes, for seed selection.
    pub fn category(self) -> &'static str {
        match self {
            Self::EntryPoint | Self::BootMain => "main",
            Self::BootRunApp => "runapp",
            Self::BootstrapInit => "init",
            Self::DeepLinkHandler => "deeplink",
            Self::ActivityHandler => "activity",
        }
    }
}

/// One host-derived claim about a selector, and optionally about an address.
#[derive(Debug, Clone, PartialEq)]
pub struct Hint {
    pub kind: HintKind,
    pub origin: HintOrigin,
    pub provenance: HintProvenance,
    /// The selector or class-like token the hint is about.
    pub selector: String,
    /// The code address the hint points at, when one is known. A hint with no
    /// address still steers naming; it just cannot steer seed selection.
    pub target_va: Option<u64>,
    /// The owning class as the model reports it, when the hint came from a
    /// model record. Never invented.
    pub owner_class: Option<String>,
    pub library_uri: Option<String>,
    /// Free text for the report. Nothing keys on it.
    pub detail: String,
}

impl Hint {
    fn key(&self) -> (HintKind, HintOrigin, String, Option<u64>) {
        (
            self.kind,
            self.origin,
            self.selector.to_ascii_lowercase(),
            self.target_va,
        )
    }
}

/// The hints one analysis run accumulated.
///
/// Insertion is deduplicating, so running an enrichment pass twice is a no-op
/// rather than a doubling. Order is insertion order, which keeps reports stable
/// for a given input.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProgramHints {
    entries: Vec<Hint>,
    seen: BTreeSet<(HintKind, HintOrigin, String, Option<u64>)>,
}

impl ProgramHints {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a hint. Returns whether it was new.
    pub fn push(&mut self, hint: Hint) -> bool {
        if !self.seen.insert(hint.key()) {
            return false;
        }
        self.entries.push(hint);
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = &Hint> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn of_kind(&self, kind: HintKind) -> impl Iterator<Item = &Hint> {
        self.entries.iter().filter(move |h| h.kind == kind)
    }

    /// The bootflow categories claimed for one address.
    pub fn categories_for_va(&self, va: u64) -> BTreeSet<&'static str> {
        self.entries
            .iter()
            .filter(|h| h.target_va == Some(va))
            .map(|h| h.kind.category())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hint(kind: HintKind, selector: &str, va: Option<u64>) -> Hint {
        Hint {
            kind,
            origin: HintOrigin::AndroidManifest,
            provenance: HintProvenance::Derived,
            selector: selector.to_string(),
            target_va: va,
            owner_class: None,
            library_uri: None,
            detail: String::new(),
        }
    }

    #[test]
    fn pushing_the_same_hint_twice_adds_one_record() {
        let mut hints = ProgramHints::new();
        assert!(hints.push(hint(HintKind::EntryPoint, "main", Some(0x1000))));
        assert!(!hints.push(hint(HintKind::EntryPoint, "MAIN", Some(0x1000))));
        assert_eq!(hints.len(), 1);
    }

    #[test]
    fn categories_are_collected_per_address() {
        let mut hints = ProgramHints::new();
        hints.push(hint(HintKind::BootMain, "main", Some(0x1000)));
        hints.push(hint(HintKind::ActivityHandler, "onNewIntent", Some(0x1000)));
        hints.push(hint(HintKind::BootRunApp, "runApp", Some(0x2000)));
        assert_eq!(
            hints
                .categories_for_va(0x1000)
                .into_iter()
                .collect::<Vec<_>>(),
            vec!["activity", "main"]
        );
        assert_eq!(
            hints
                .categories_for_va(0x2000)
                .into_iter()
                .collect::<Vec<_>>(),
            vec!["runapp"]
        );
        assert!(hints.categories_for_va(0x3000).is_empty());
    }
}
