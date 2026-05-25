//! Typed Synthesizer trait — multi-target emission for one Architecture.
//!
//! Decouples authoring from serialization. Same `Architecture` value
//! emits to:
//!
//! - [`TerraformJson`] — wire-compatible with tofu / magma / every
//!   tfplugin5/6 provider (existing).
//! - [`MagmaPlan`] — direct typed `magma_types::Plan` (planned: skips
//!   the JSON intermediate when magma is in-process).
//! - [`CrossplaneYaml`] — Kubernetes-native IaC (planned).
//! - [`PulumiYaml`] — Pulumi YAML automation API (planned).
//!
//! Every future emission target slots in by adding `impl Synthesizer<NewTarget>
//! for Architecture { ... }`. The architecture corpus (88+ ports from
//! pangea-architectures) automatically gets all targets for free —
//! one rule, multiple realizations.

use crate::{Architecture, RenderError};

/// Typed render target. Each target is a zero-size marker type the
/// generic [`Synthesizer`] impl dispatches on.
pub trait RenderTarget {
    /// The typed value produced by rendering to this target.
    type Output;
}

/// Multi-target emitter. One trait per (Source, Target) pair; one
/// generic implementor — [`Architecture`] — owns every target.
pub trait Synthesizer<T: RenderTarget> {
    fn synthesize(&self) -> Result<T::Output, RenderError>;
}

// ── Terraform JSON target ────────────────────────────────────────────

/// Marker: emit `terraform.json`. Wire-compatible with tofu + magma.
#[derive(Debug, Clone, Copy)]
pub struct TerraformJson;

impl RenderTarget for TerraformJson {
    type Output = serde_json::Value;
}

impl Synthesizer<TerraformJson> for Architecture {
    fn synthesize(&self) -> Result<serde_json::Value, RenderError> {
        self.render_terraform_json()
    }
}

// ── (planned) Direct magma::Plan target ─────────────────────────────
// Stub marker — fills in when magma-types crate is available as a dep.
// Architecture → magma_types::Plan skipping the JSON intermediate.

/// Marker: emit a typed magma::Plan directly (skips JSON).
/// Implementation lands when magma-types is a lava-core dep.
#[derive(Debug, Clone, Copy)]
pub struct MagmaPlan;

impl RenderTarget for MagmaPlan {
    /// Held opaquely as JSON until magma-types is wired in. Magma's
    /// existing `magma_config::config_from_terraform_json` already
    /// produces a Plan from this value — no behavior change at this
    /// stage; the marker reserves the type slot for the M1 swap-in.
    type Output = serde_json::Value;
}

impl Synthesizer<MagmaPlan> for Architecture {
    fn synthesize(&self) -> Result<serde_json::Value, RenderError> {
        // For now identical to TerraformJson — magma consumes the JSON
        // through its existing config-loading path. Swap to direct
        // Plan construction when magma-types is a lava-core dep.
        self.render_terraform_json()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Architecture, Resource, Value};
    use indexmap::IndexMap;

    fn tiny_vpc() -> Architecture {
        let mut arch = Architecture::new("vpc");
        let mut attrs = IndexMap::new();
        attrs.insert("cidr_block".to_string(), Value::s("10.0.0.0/16"));
        arch.resources.push(Resource {
            type_id: "aws_vpc".to_string(),
            name: "main".to_string(),
            attributes: attrs,
            depends_on: vec![],
            provider: None,
            multiplicity: None,
        });
        arch
    }

    #[test]
    fn synthesize_to_terraform_json_via_trait() {
        let arch = tiny_vpc();
        let json: serde_json::Value = Synthesizer::<TerraformJson>::synthesize(&arch).unwrap();
        assert_eq!(json["resource"]["aws_vpc"]["main"]["cidr_block"], "10.0.0.0/16");
    }

    #[test]
    fn synthesize_to_magma_plan_via_trait() {
        let arch = tiny_vpc();
        // M0 behavior: identical to TerraformJson. M1: typed Plan directly.
        let plan: serde_json::Value = Synthesizer::<MagmaPlan>::synthesize(&arch).unwrap();
        assert_eq!(plan["resource"]["aws_vpc"]["main"]["cidr_block"], "10.0.0.0/16");
    }
}
