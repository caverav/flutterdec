//! Newtypes the adapter boundary refuses to accept as bare strings.
//!
//! Both of these exist because the values they wrap arrive from outside the
//! host: a digest that is really a filename, or a path that is really `../..`,
//! is not a validation problem to be caught later but a value that must never
//! be constructed. Validation therefore lives in `Deserialize`, so parsing
//! untrusted JSON either yields a usable value or fails.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

/// A lowercase hex SHA-256.
///
/// The snapshot hash is a compatibility fingerprint and is deliberately *not*
/// this type; these are content digests over bytes the host itself read.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Sha256Digest(String);

impl Sha256Digest {
    pub fn of(bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        Self(format!("{:x}", hasher.finalize()))
    }

    pub fn parse(text: &str) -> Result<Self, PrimitiveError> {
        if text.len() != 64 {
            return Err(PrimitiveError::DigestLength(text.len()));
        }
        if !text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(PrimitiveError::DigestAlphabet(text.to_string()));
        }
        Ok(Self(text.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Sha256Digest {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// A path handle that is relative, contained, and free of traversal.
///
/// The adapter receives one of these per input and one for its output. Nothing
/// downstream re-checks them, so the containment guarantee has to hold from the
/// moment the value exists: no absolute path, no `..`, no `.`, no empty or
/// repeated separator, no backslash, no drive prefix, no NUL.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct RelativePath(String);

impl RelativePath {
    pub fn parse(text: &str) -> Result<Self, PrimitiveError> {
        if text.is_empty() {
            return Err(PrimitiveError::PathEmpty);
        }
        // A backslash is a separator on some hosts and an ordinary character on
        // others, so a path containing one cannot mean the same thing to the
        // host and the adapter. Refuse rather than pick an interpretation.
        if text.contains('\\') {
            return Err(PrimitiveError::PathBackslash(text.to_string()));
        }
        if text.contains('\0') {
            return Err(PrimitiveError::PathNul(text.to_string()));
        }
        if text.starts_with('/') {
            return Err(PrimitiveError::PathAbsolute(text.to_string()));
        }
        // `C:` and friends are absolute even without a leading separator.
        if text.len() >= 2 && text.as_bytes()[1] == b':' {
            return Err(PrimitiveError::PathAbsolute(text.to_string()));
        }
        for component in text.split('/') {
            if component.is_empty() {
                return Err(PrimitiveError::PathEmptyComponent(text.to_string()));
            }
            if component == "." || component == ".." {
                return Err(PrimitiveError::PathTraversal(text.to_string()));
            }
        }
        Ok(Self(text.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RelativePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RelativePath {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrimitiveError {
    DigestLength(usize),
    DigestAlphabet(String),
    PathEmpty,
    PathAbsolute(String),
    PathTraversal(String),
    PathEmptyComponent(String),
    PathBackslash(String),
    PathNul(String),
}

impl fmt::Display for PrimitiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DigestLength(len) => {
                write!(f, "sha-256 digest must be 64 hex characters, got {}", len)
            }
            Self::DigestAlphabet(text) => {
                write!(f, "sha-256 digest must be lowercase hex, got {:?}", text)
            }
            Self::PathEmpty => f.write_str("path handle must not be empty"),
            Self::PathAbsolute(text) => write!(f, "path handle {:?} is absolute", text),
            Self::PathTraversal(text) => {
                write!(f, "path handle {:?} contains a traversal component", text)
            }
            Self::PathEmptyComponent(text) => {
                write!(f, "path handle {:?} has an empty component", text)
            }
            Self::PathBackslash(text) => write!(f, "path handle {:?} contains a backslash", text),
            Self::PathNul(text) => write!(f, "path handle {:?} contains a NUL", text),
        }
    }
}

impl std::error::Error for PrimitiveError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digests_are_lowercase_hex_of_the_right_length() {
        let digest = Sha256Digest::of(b"");
        assert_eq!(
            digest.as_str(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(Sha256Digest::parse(digest.as_str()).unwrap(), digest);
    }

    #[test]
    fn a_digest_that_is_not_a_digest_is_rejected() {
        assert_eq!(
            Sha256Digest::parse("deadbeef"),
            Err(PrimitiveError::DigestLength(8))
        );
        // Uppercase is a different string, and accepting it would make two
        // spellings of one digest compare unequal.
        let upper = "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855";
        assert!(matches!(
            Sha256Digest::parse(upper),
            Err(PrimitiveError::DigestAlphabet(_))
        ));
        assert!(matches!(
            Sha256Digest::parse(&"z".repeat(64)),
            Err(PrimitiveError::DigestAlphabet(_))
        ));
    }

    #[test]
    fn path_handles_stay_relative_and_contained() {
        assert_eq!(
            RelativePath::parse("in/vm_data.bin").unwrap().as_str(),
            "in/vm_data.bin"
        );
        for (text, expected) in [
            ("", PrimitiveError::PathEmpty),
            (
                "/etc/passwd",
                PrimitiveError::PathAbsolute("/etc/passwd".into()),
            ),
            ("C:/x", PrimitiveError::PathAbsolute("C:/x".into())),
            (
                "../escape",
                PrimitiveError::PathTraversal("../escape".into()),
            ),
            ("a/../b", PrimitiveError::PathTraversal("a/../b".into())),
            ("./a", PrimitiveError::PathTraversal("./a".into())),
            ("a//b", PrimitiveError::PathEmptyComponent("a//b".into())),
            ("a/", PrimitiveError::PathEmptyComponent("a/".into())),
            ("a\\b", PrimitiveError::PathBackslash("a\\b".into())),
            ("a\0b", PrimitiveError::PathNul("a\0b".into())),
        ] {
            assert_eq!(RelativePath::parse(text), Err(expected), "input {:?}", text);
        }
    }

    /// The guarantee has to survive deserialization, which is the only way these
    /// values actually enter the process.
    #[test]
    fn fresh_json_cannot_smuggle_a_traversal_or_a_short_digest() {
        assert!(serde_json::from_str::<RelativePath>("\"../../etc/passwd\"").is_err());
        assert!(serde_json::from_str::<Sha256Digest>("\"nope\"").is_err());
        let ok: RelativePath = serde_json::from_str("\"out/model.json\"").unwrap();
        assert_eq!(ok.as_str(), "out/model.json");
    }
}
