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

// ── Crossplane Composition + XRD target ─────────────────────────────

/// Marker: emit a Crossplane Composition (per-resource template) plus
/// a paired CompositeResourceDefinition (typed input schema). The
/// pair is what Kubernetes operators submit to a cluster running the
/// crossplane controller.
///
/// Output shape:
///
/// ```yaml
/// apiVersion: apiextensions.crossplane.io/v1
/// kind: CompositeResourceDefinition
/// metadata: { name: <arch>.lava.pleme.io }
/// spec:
///   group: lava.pleme.io
///   names: { kind: <ArchKind>, plural: <archs> }
///   versions: [ ... ]
/// ---
/// apiVersion: apiextensions.crossplane.io/v1
/// kind: Composition
/// metadata: { name: <arch>-composition }
/// spec:
///   compositeTypeRef: { ... }
///   resources: [ <one entry per Architecture resource> ]
/// ```
#[derive(Debug, Clone, Copy)]
pub struct CrossplaneYaml;

impl RenderTarget for CrossplaneYaml {
    type Output = String;
}

impl Synthesizer<CrossplaneYaml> for Architecture {
    fn synthesize(&self) -> Result<String, RenderError> {
        let json = self.render_terraform_json()?;
        Ok(crossplane_yaml_from(&self.name, &json))
    }
}

/// Build the typed Crossplane YAML pair from a typed terraform.json
/// shape. Internal helper — exposed via `Synthesizer<CrossplaneYaml>`.
fn crossplane_yaml_from(arch_name: &str, terraform: &serde_json::Value) -> String {
    let kind_name = arch_kind(arch_name);
    let mut xrd = serde_yaml::Mapping::new();
    xrd.insert("apiVersion".into(), "apiextensions.crossplane.io/v1".into());
    xrd.insert("kind".into(), "CompositeResourceDefinition".into());
    let mut xrd_metadata = serde_yaml::Mapping::new();
    xrd_metadata.insert(
        "name".into(),
        serde_yaml::Value::String(format!(
            "{}.lava.pleme.io",
            arch_name.to_ascii_lowercase()
        )),
    );
    xrd.insert("metadata".into(), serde_yaml::Value::Mapping(xrd_metadata));
    let mut xrd_spec = serde_yaml::Mapping::new();
    xrd_spec.insert("group".into(), "lava.pleme.io".into());
    let mut xrd_names = serde_yaml::Mapping::new();
    xrd_names.insert("kind".into(), serde_yaml::Value::String(kind_name.clone()));
    xrd_names.insert(
        "plural".into(),
        serde_yaml::Value::String(format!("{}s", kind_name.to_ascii_lowercase())),
    );
    xrd_spec.insert("names".into(), serde_yaml::Value::Mapping(xrd_names));
    xrd.insert("spec".into(), serde_yaml::Value::Mapping(xrd_spec));

    let mut composition = serde_yaml::Mapping::new();
    composition.insert("apiVersion".into(), "apiextensions.crossplane.io/v1".into());
    composition.insert("kind".into(), "Composition".into());
    let mut comp_metadata = serde_yaml::Mapping::new();
    comp_metadata.insert(
        "name".into(),
        serde_yaml::Value::String(format!("{arch_name}-composition")),
    );
    composition.insert("metadata".into(), serde_yaml::Value::Mapping(comp_metadata));
    let mut comp_spec = serde_yaml::Mapping::new();
    let mut composite_type_ref = serde_yaml::Mapping::new();
    composite_type_ref.insert("apiVersion".into(), "lava.pleme.io/v1alpha1".into());
    composite_type_ref.insert("kind".into(), serde_yaml::Value::String(kind_name));
    comp_spec.insert(
        "compositeTypeRef".into(),
        serde_yaml::Value::Mapping(composite_type_ref),
    );

    // One Composition.resources entry per terraform resource — the
    // Crossplane provider routes each to its matching provider.
    let mut resources = Vec::new();
    if let Some(by_type) = terraform.get("resource").and_then(serde_json::Value::as_object) {
        for (type_id, by_name) in by_type {
            if let Some(by_name_map) = by_name.as_object() {
                for (name, body) in by_name_map {
                    let mut entry = serde_yaml::Mapping::new();
                    entry.insert(
                        "name".into(),
                        serde_yaml::Value::String(format!("{type_id}.{name}")),
                    );
                    let mut base = serde_yaml::Mapping::new();
                    base.insert(
                        "apiVersion".into(),
                        serde_yaml::Value::String(format!("{type_id}.crossplane.io/v1alpha1")),
                    );
                    base.insert("kind".into(), serde_yaml::Value::String(arch_kind(type_id)));
                    let mut base_metadata = serde_yaml::Mapping::new();
                    base_metadata.insert("name".into(), serde_yaml::Value::String(name.clone()));
                    base.insert("metadata".into(), serde_yaml::Value::Mapping(base_metadata));
                    let mut spec = serde_yaml::Mapping::new();
                    spec.insert(
                        "forProvider".into(),
                        json_to_yaml(body),
                    );
                    base.insert("spec".into(), serde_yaml::Value::Mapping(spec));
                    entry.insert("base".into(), serde_yaml::Value::Mapping(base));
                    resources.push(serde_yaml::Value::Mapping(entry));
                }
            }
        }
    }
    comp_spec.insert("resources".into(), serde_yaml::Value::Sequence(resources));
    composition.insert("spec".into(), serde_yaml::Value::Mapping(comp_spec));

    // Multi-document YAML emission: typed MultiDocYaml wrapper with
    // a Display impl that writes `---\n` between documents via
    // writeln!() (the allowed surface per ★★ TYPED EMISSION).
    MultiDocYaml(vec![
        serde_yaml::Value::Mapping(xrd),
        serde_yaml::Value::Mapping(composition),
    ])
    .to_string()
}

/// Typed multi-document YAML — joins N serde_yaml::Value documents
/// with the YAML `---` separator via the Display surface. Avoids
/// format!()-of-YAML-syntax per ★★ TYPED EMISSION.
struct MultiDocYaml(Vec<serde_yaml::Value>);

impl std::fmt::Display for MultiDocYaml {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, doc) in self.0.iter().enumerate() {
            if i > 0 {
                writeln!(f, "---")?;
            }
            // serde_yaml::to_string already terminates with a newline;
            // write the raw serialization through.
            let body = serde_yaml::to_string(doc).unwrap_or_default();
            f.write_str(&body)?;
        }
        Ok(())
    }
}

/// Convert `aws-vpc-network` → `AwsVpcNetwork` (Crossplane Kind shape).
fn arch_kind(arch_name: &str) -> String {
    let mut out = String::with_capacity(arch_name.len());
    let mut capitalize_next = true;
    for c in arch_name.chars() {
        if c == '-' || c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            out.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn json_to_yaml(v: &serde_json::Value) -> serde_yaml::Value {
    match v {
        serde_json::Value::Null => serde_yaml::Value::Null,
        serde_json::Value::Bool(b) => serde_yaml::Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_yaml::Value::Number(serde_yaml::Number::from(i))
            } else if let Some(f) = n.as_f64() {
                serde_yaml::Value::Number(serde_yaml::Number::from(f))
            } else {
                serde_yaml::Value::Null
            }
        }
        serde_json::Value::String(s) => serde_yaml::Value::String(s.clone()),
        serde_json::Value::Array(items) => {
            serde_yaml::Value::Sequence(items.iter().map(json_to_yaml).collect())
        }
        serde_json::Value::Object(map) => {
            let mut m = serde_yaml::Mapping::new();
            for (k, val) in map {
                m.insert(serde_yaml::Value::String(k.clone()), json_to_yaml(val));
            }
            serde_yaml::Value::Mapping(m)
        }
    }
}

// ── Grafana dashboard JSON target ───────────────────────────────────

/// Marker: emit a Grafana dashboard document (schemaVersion 39).
///
/// The first target whose *source* is not an [`Architecture`]. The trait
/// is generic over the target and implemented per (source, target) pair,
/// so a second document type slots in exactly the way the header
/// promises — `impl Synthesizer<NewTarget> for NewSource`.
///
/// Rendering needs a [`Theme`](crate::Theme) to resolve semantic colour
/// roles, and `Synthesizer::synthesize` takes no arguments, so this impl
/// uses the fleet default. Call
/// [`Dashboard::render_grafana_json`](crate::Dashboard::render_grafana_json)
/// directly to render against a different theme.
#[derive(Debug, Clone, Copy)]
pub struct GrafanaJson;

impl RenderTarget for GrafanaJson {
    type Output = serde_json::Value;
}

impl Synthesizer<GrafanaJson> for crate::Dashboard {
    fn synthesize(&self) -> Result<serde_json::Value, RenderError> {
        self.render_grafana_json(&crate::Theme::default())
            .map_err(|e| RenderError::Invalid(e.to_string()))
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

    #[test]
    fn synthesize_to_crossplane_yaml_emits_xrd_and_composition() {
        let arch = tiny_vpc();
        let yaml: String = Synthesizer::<CrossplaneYaml>::synthesize(&arch).unwrap();

        // CompositeResourceDefinition for the architecture.
        assert!(yaml.contains("kind: CompositeResourceDefinition"));
        assert!(yaml.contains("name: vpc.lava.pleme.io"));
        // Composition referencing the kind.
        assert!(yaml.contains("kind: Composition"));
        assert!(yaml.contains("name: vpc-composition"));
        // The resource entry is name-prefixed and carries the
        // forProvider block with the original cidr_block value.
        assert!(yaml.contains("name: aws_vpc.main"));
        assert!(yaml.contains("cidr_block: 10.0.0.0/16"));
        // Documents are split by ---.
        assert!(yaml.contains("---"));
    }

    #[test]
    fn arch_kind_converts_kebab_to_pascal() {
        assert_eq!(arch_kind("aws-vpc-network"), "AwsVpcNetwork");
        assert_eq!(arch_kind("cloudflare_dns_records"), "CloudflareDnsRecords");
        assert_eq!(arch_kind("simple"), "Simple");
    }

    #[test]
    fn crossplane_yaml_parses_as_two_valid_yaml_documents() {
        use serde::Deserialize;
        let arch = tiny_vpc();
        let yaml = Synthesizer::<CrossplaneYaml>::synthesize(&arch).unwrap();
        let docs: Vec<serde_yaml::Value> = serde_yaml::Deserializer::from_str(&yaml)
            .map(|d| serde_yaml::Value::deserialize(d).unwrap())
            .collect();
        assert_eq!(docs.len(), 2, "expected XRD + Composition pair");
        assert_eq!(docs[0]["kind"], "CompositeResourceDefinition");
        assert_eq!(docs[1]["kind"], "Composition");
    }
}
