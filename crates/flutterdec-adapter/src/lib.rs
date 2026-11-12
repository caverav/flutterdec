//! Host side of the adapter boundary.
//!
//! One adapter run is one process: the host writes the snapshot regions and an
//! [`protocol::AdapterRequest`] into a private invocation directory, runs the
//! adapter there under an explicit set of limits, and reads back an
//! [`protocol::AdapterResult`] plus a [`model::ProgramModel`]. Nothing about the
//! run is decided by the adapter: the identity, the registry record, the
//! producer record, the compatibility binding, and the region table are host
//! facts, and every one of them is checked before a process exists.
//!
//! There is no v2/v3 path. [`model::ProgramModel::from_json`] rejects those
//! documents by version, so an old adapter fails loudly instead of being
//! silently reinterpreted.

pub mod host;
pub mod model;
pub mod primitives;
pub mod protocol;
pub mod sandbox;
pub mod store;
pub mod validate;
/// Host compatibility records live in the loader crate so profile and identity
/// selection cannot depend on adapter model DTOs; re-export them at the adapter
/// boundary for callers that own adapter lifecycle.
pub mod registry {
    pub use flutterdec_loader::registry::*;
}

pub use host::{
    run_adapter, AdapterInput, AdapterRegionInput, AdapterRun, HostAuthorization, HostError,
    LibappSource, OutputStream,
};
pub use sandbox::{ContainmentReport, ControlState, Limits};

use flutterdec_loader::identity::IdentityRejection;

/// Wrap an identity rejection so it survives as a typed cause.
///
/// `anyhow::Error::new` keeps the `IdentityRejection` downcastable, so a caller
/// can act on *which* check refused the snapshot rather than parse a message.
pub fn identity_rejected(rejection: IdentityRejection) -> anyhow::Error {
    anyhow::Error::new(rejection).context("snapshot identity may not authorize an adapter")
}
