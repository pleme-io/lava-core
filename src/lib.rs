//! lava-core — typed primitive layer for the lava suite.
//!
//! The tatara-lisp DSL frontend that sits over pleme-io/magma the way
//! pangea-core sits over pangea-forge → terraform-synthesizer →
//! tofu/terraform. Lava (Brazilian-Portuguese for the substance magma
//! flows as) is the surface operators author; magma is the executor.
//!
//! ## Pipeline
//!
//! ```text
//! tatara-lisp source     `(deflava-architecture my-vpc ...)`
//!         │
//!         ▼  tatara-vm evaluates the (deflava-*) forms
//! lava::Architecture     pure Rust typed value (this crate)
//!         │
//!         ▼  Architecture::render_terraform_json()
//! terraform.json         wire-compatible with magma + every TF provider
//!         │
//!         ▼  magma applies via gRPC providers
//! cloud resources
//! ```
//!
//! ## Typed surface
//!
//! - [`Resource`] — one typed resource, e.g. `aws_vpc.main`.
//! - [`Architecture`] — composition of resources + outputs.
//! - [`Stack`] — deployment instance of an architecture into a backend.
//! - [`ProviderRef`] — provider+alias the resource is materialized through.
//!
//! No format!() of code; every JSON serialization goes through serde +
//! the typed render impl. The lib is host-side (tatara-vm runs it), not
//! a tatara-script primitive itself.

#![allow(clippy::module_name_repetitions)]

pub mod synthesizer;
pub use synthesizer::{CrossplaneYaml, MagmaPlan, RenderTarget, Synthesizer, TerraformJson};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Provider reference: namespace + name + optional alias. Same shape
/// magma uses (`magma_types::ProviderReference`); field set kept
/// identical for round-trip-byte-equal interop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderRef {
    /// e.g. "hashicorp/aws"
    pub source: String,
    /// e.g. "aws"
    pub name: String,
    /// e.g. "us-east-2" — None for the default unaliased provider.
    pub alias: Option<String>,
    /// Provider-level configuration (region, profile, default tags,
    /// auth fields). Renders inside `provider.<name>.{…}`.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub config: IndexMap<String, Value>,
}

/// Reference to another resource's output. Renders as
/// `${aws_vpc.main.id}` in Terraform JSON; held typed in-memory so
/// downstream tooling (graph builder, change detector) can walk the
/// dep graph without parsing interpolation strings.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourceRef {
    pub type_id: String,
    pub name: String,
    pub attribute: String,
}

/// Attribute value. Wraps serde_json::Value plus a typed variant for
/// references — keeps the dep graph walkable without string-parsing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Ref(ResourceRef),
    Json(serde_json::Value),
}

impl Value {
    #[must_use]
    pub fn s(v: impl Into<String>) -> Self {
        Self::Json(serde_json::Value::String(v.into()))
    }
    #[must_use]
    pub fn b(v: bool) -> Self {
        Self::Json(serde_json::Value::Bool(v))
    }
    #[must_use]
    pub fn n(v: i64) -> Self {
        Self::Json(serde_json::Value::Number(v.into()))
    }
    #[must_use]
    pub fn arr(items: impl IntoIterator<Item = Value>) -> Self {
        Self::Json(serde_json::Value::Array(
            items.into_iter().map(Value::into_json).collect(),
        ))
    }
    /// Strip the typed wrapper for JSON emission. References serialize
    /// as `${type.name.attribute}` per Terraform's interpolation syntax.
    #[must_use]
    pub fn into_json(self) -> serde_json::Value {
        match self {
            Self::Json(v) => v,
            Self::Ref(r) => {
                let mut s = String::from("${");
                s.push_str(&r.type_id);
                s.push('.');
                s.push_str(&r.name);
                s.push('.');
                s.push_str(&r.attribute);
                s.push('}');
                serde_json::Value::String(s)
            }
        }
    }
}

/// Resource multiplicity. Terraform exposes count (numeric) and
/// for_each (string-keyed map). Lava represents both with one typed
/// enum; renderer emits the correct wire field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Multiplicity {
    Count(i64),
    ForEach(IndexMap<String, Value>),
}

/// One typed resource. `(deflava-resource :aws-vpc "main" :cidr "10.0.0.0/16")`
/// authored as tatara-lisp lands here as a `Resource` value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resource {
    /// Resource type — Terraform `type` field, e.g. `aws_vpc`.
    pub type_id: String,
    /// Logical name within the architecture — Terraform `name`.
    pub name: String,
    /// Attribute map. Order-preserved for byte-equal JSON round-trip.
    pub attributes: IndexMap<String, Value>,
    /// Optional explicit dependencies (Terraform `depends_on`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<ResourceRef>,
    /// Optional provider override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderRef>,
    /// Count / for_each — None means single instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiplicity: Option<Multiplicity>,
}

impl Resource {
    /// Get a typed reference to one of this resource's output attributes.
    /// Substitutes for ad-hoc string templating elsewhere in user code.
    #[must_use]
    pub fn out(&self, attribute: impl Into<String>) -> ResourceRef {
        ResourceRef {
            type_id: self.type_id.clone(),
            name: self.name.clone(),
            attribute: attribute.into(),
        }
    }
}

/// Composition holder. Maps to one Terraform configuration (root or
/// sub-module). Outputs are the architecture's typed contract —
/// downstream architectures consume `Architecture::output("vpc_id")`
/// the same way pangea consumers consume `NetworkResult#vpc.id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Architecture {
    pub name: String,
    pub resources: Vec<Resource>,
    /// `data` blocks — terraform `data.<type>.<name>`. Same shape as
    /// resources (typed key/value attributes); the renderer routes
    /// them under the top-level `data` key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_sources: Vec<Resource>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub outputs: IndexMap<String, Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<ProviderRef>,
    /// `locals` block — terraform `locals { … }`. Authoring-side
    /// constants downstream `${local.foo}` references resolve against.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub locals: IndexMap<String, Value>,
}

impl Architecture {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            resources: Vec::new(),
            data_sources: Vec::new(),
            outputs: IndexMap::new(),
            providers: Vec::new(),
            locals: IndexMap::new(),
        }
    }

    /// Push a typed resource into the architecture. Returns the typed
    /// reference for downstream consumption (`vpc.out("id")` etc.).
    pub fn add(&mut self, r: Resource) -> ResourceRef {
        let rref = r.out("id");
        self.resources.push(r);
        rref
    }

    /// Declare an output. The output value flows to downstream
    /// architectures + remote-state-consumer modules.
    pub fn output(&mut self, key: impl Into<String>, value: Value) {
        self.outputs.insert(key.into(), value);
    }

    /// Render to Terraform JSON. Wire-compatible with magma + every
    /// tfplugin5/6 provider. Output is byte-stable across runs when
    /// inputs are byte-stable (IndexMap preserves insertion order).
    pub fn render_terraform_json(&self) -> Result<serde_json::Value, RenderError> {
        let mut root = serde_json::Map::new();

        // provider block — Terraform JSON allows multiple configs per
        // provider via array form (`provider: { aws: [{...}, {...}] }`).
        if !self.providers.is_empty() {
            let mut grouped: IndexMap<String, Vec<serde_json::Value>> = IndexMap::new();
            for p in &self.providers {
                let mut entry = serde_json::Map::new();
                if let Some(alias) = &p.alias {
                    entry.insert("alias".to_string(), serde_json::Value::String(alias.clone()));
                }
                for (k, v) in &p.config {
                    entry.insert(k.clone(), v.clone().into_json());
                }
                grouped
                    .entry(p.name.clone())
                    .or_default()
                    .push(serde_json::Value::Object(entry));
            }
            let mut provider_map = serde_json::Map::new();
            for (name, mut entries) in grouped {
                // Single config → object; multiple → array (terraform-spec).
                if entries.len() == 1 {
                    provider_map.insert(name, entries.remove(0));
                } else {
                    provider_map.insert(name, serde_json::Value::Array(entries));
                }
            }
            root.insert("provider".to_string(), serde_json::Value::Object(provider_map));
        }

        // locals block
        if !self.locals.is_empty() {
            let mut locals = serde_json::Map::new();
            for (k, v) in &self.locals {
                locals.insert(k.clone(), v.clone().into_json());
            }
            root.insert("locals".to_string(), serde_json::Value::Object(locals));
        }

        // resource block — nested as resource.<type>.<name> = {attrs}
        let mut by_type: IndexMap<String, IndexMap<String, serde_json::Value>> = IndexMap::new();
        for r in &self.resources {
            let mut body = serde_json::Map::new();
            for (k, v) in &r.attributes {
                body.insert(k.clone(), v.clone().into_json());
            }
            if let Some(m) = &r.multiplicity {
                match m {
                    Multiplicity::Count(n) => {
                        body.insert(
                            "count".to_string(),
                            serde_json::Value::Number((*n).into()),
                        );
                    }
                    Multiplicity::ForEach(map) => {
                        let mut for_each_map = serde_json::Map::new();
                        for (k, v) in map {
                            for_each_map.insert(k.clone(), v.clone().into_json());
                        }
                        body.insert(
                            "for_each".to_string(),
                            serde_json::Value::Object(for_each_map),
                        );
                    }
                }
            }
            if !r.depends_on.is_empty() {
                let deps: Vec<serde_json::Value> = r
                    .depends_on
                    .iter()
                    .map(|d| {
                        let mut s = d.type_id.clone();
                        s.push('.');
                        s.push_str(&d.name);
                        serde_json::Value::String(s)
                    })
                    .collect();
                body.insert("depends_on".to_string(), serde_json::Value::Array(deps));
            }
            by_type
                .entry(r.type_id.clone())
                .or_default()
                .insert(r.name.clone(), serde_json::Value::Object(body));
        }
        if !by_type.is_empty() {
            let mut resource = serde_json::Map::new();
            for (type_id, named) in by_type {
                let mut t = serde_json::Map::new();
                for (n, body) in named {
                    t.insert(n, body);
                }
                resource.insert(type_id, serde_json::Value::Object(t));
            }
            root.insert("resource".to_string(), serde_json::Value::Object(resource));
        }

        // data block — same shape as resource, routed under `data`.
        if !self.data_sources.is_empty() {
            let mut data_by_type: IndexMap<String, IndexMap<String, serde_json::Value>> =
                IndexMap::new();
            for d in &self.data_sources {
                let mut body = serde_json::Map::new();
                for (k, v) in &d.attributes {
                    body.insert(k.clone(), v.clone().into_json());
                }
                data_by_type
                    .entry(d.type_id.clone())
                    .or_default()
                    .insert(d.name.clone(), serde_json::Value::Object(body));
            }
            let mut data = serde_json::Map::new();
            for (type_id, named) in data_by_type {
                let mut t = serde_json::Map::new();
                for (n, body) in named {
                    t.insert(n, body);
                }
                data.insert(type_id, serde_json::Value::Object(t));
            }
            root.insert("data".to_string(), serde_json::Value::Object(data));
        }

        // output block
        if !self.outputs.is_empty() {
            let mut outputs = serde_json::Map::new();
            for (k, v) in &self.outputs {
                let mut entry = serde_json::Map::new();
                entry.insert("value".to_string(), v.clone().into_json());
                outputs.insert(k.clone(), serde_json::Value::Object(entry));
            }
            root.insert("output".to_string(), serde_json::Value::Object(outputs));
        }

        Ok(serde_json::Value::Object(root))
    }
}

/// Deployment instance. Same architecture can be stacked into many
/// environments (prod/staging/dev) by varying backend + workspace +
/// variable overrides. Maps to one Terraform workspace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stack {
    pub name: String,
    pub architecture: Architecture,
    pub workspace: String,
    pub backend: BackendRef,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub variable_overrides: IndexMap<String, Value>,
}

/// State backend reference. Local file / S3 / GCS / Azure blob. The
/// backend tells magma where state lives + how to lock it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BackendRef {
    Local {
        path: String,
    },
    S3 {
        bucket: String,
        key: String,
        region: String,
    },
    Gcs {
        bucket: String,
        prefix: String,
    },
    AzureBlob {
        storage_account: String,
        container: String,
        key: String,
    },
}

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("JSON serialization failed: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_architecture_renders_empty_json_object() {
        let arch = Architecture::new("empty");
        let json = arch.render_terraform_json().unwrap();
        assert_eq!(json, serde_json::json!({}));
    }

    #[test]
    fn single_resource_renders_terraform_shape() {
        let mut arch = Architecture::new("vpc-only");
        let mut attrs = IndexMap::new();
        attrs.insert("cidr_block".to_string(), Value::s("10.0.0.0/16"));
        attrs.insert("enable_dns_support".to_string(), Value::b(true));
        arch.resources.push(Resource {
            type_id: "aws_vpc".to_string(),
            name: "main".to_string(),
            attributes: attrs,
            depends_on: vec![],
            provider: None,
            multiplicity: None,
        });
        let json = arch.render_terraform_json().unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "resource": {
                    "aws_vpc": {
                        "main": {
                            "cidr_block": "10.0.0.0/16",
                            "enable_dns_support": true
                        }
                    }
                }
            })
        );
    }

    #[test]
    fn resource_reference_renders_as_terraform_interpolation() {
        let mut arch = Architecture::new("vpc-with-igw");
        let mut vpc_attrs = IndexMap::new();
        vpc_attrs.insert("cidr_block".to_string(), Value::s("10.0.0.0/16"));
        arch.resources.push(Resource {
            type_id: "aws_vpc".to_string(),
            name: "main".to_string(),
            attributes: vpc_attrs,
            depends_on: vec![],
            provider: None,
            multiplicity: None,
        });
        let mut igw_attrs = IndexMap::new();
        igw_attrs.insert(
            "vpc_id".to_string(),
            Value::Ref(ResourceRef {
                type_id: "aws_vpc".to_string(),
                name: "main".to_string(),
                attribute: "id".to_string(),
            }),
        );
        arch.resources.push(Resource {
            type_id: "aws_internet_gateway".to_string(),
            name: "igw".to_string(),
            attributes: igw_attrs,
            depends_on: vec![],
            provider: None,
            multiplicity: None,
        });
        let json = arch.render_terraform_json().unwrap();
        // The reference flowed through as `${aws_vpc.main.id}`.
        assert_eq!(
            json["resource"]["aws_internet_gateway"]["igw"]["vpc_id"],
            "${aws_vpc.main.id}"
        );
    }

    #[test]
    fn outputs_render_as_terraform_output_block() {
        let mut arch = Architecture::new("vpc-out");
        arch.outputs.insert(
            "vpc_id".to_string(),
            Value::Ref(ResourceRef {
                type_id: "aws_vpc".to_string(),
                name: "main".to_string(),
                attribute: "id".to_string(),
            }),
        );
        let json = arch.render_terraform_json().unwrap();
        assert_eq!(json["output"]["vpc_id"]["value"], "${aws_vpc.main.id}");
    }

    #[test]
    fn data_sources_render_under_top_level_data_block() {
        let mut arch = Architecture::new("net");
        let mut a = IndexMap::new();
        a.insert("name".to_string(), Value::s("amazon-linux-2"));
        a.insert("most_recent".to_string(), Value::b(true));
        arch.data_sources.push(Resource {
            type_id: "aws_ami".to_string(),
            name: "default".to_string(),
            attributes: a,
            depends_on: vec![],
            provider: None,
            multiplicity: None,
        });
        let json = arch.render_terraform_json().unwrap();
        assert_eq!(json["data"]["aws_ami"]["default"]["name"], "amazon-linux-2");
        assert_eq!(json["data"]["aws_ami"]["default"]["most_recent"], true);
    }

    #[test]
    fn locals_render_under_top_level_locals_block() {
        let mut arch = Architecture::new("net");
        arch.locals.insert("env".to_string(), Value::s("prod"));
        arch.locals.insert("retries".to_string(), Value::n(3));
        let json = arch.render_terraform_json().unwrap();
        assert_eq!(json["locals"]["env"], "prod");
        assert_eq!(json["locals"]["retries"], 3);
    }

    #[test]
    fn providers_with_config_render_inside_provider_block() {
        let mut arch = Architecture::new("net");
        let mut cfg = IndexMap::new();
        cfg.insert("region".to_string(), Value::s("us-east-2"));
        cfg.insert("profile".to_string(), Value::s("prod"));
        arch.providers.push(ProviderRef {
            source: "hashicorp/aws".to_string(),
            name: "aws".to_string(),
            alias: None,
            config: cfg,
        });
        let json = arch.render_terraform_json().unwrap();
        assert_eq!(json["provider"]["aws"]["region"], "us-east-2");
        assert_eq!(json["provider"]["aws"]["profile"], "prod");
    }

    #[test]
    fn multiple_provider_configs_render_as_array() {
        let mut arch = Architecture::new("net");
        let mut east = IndexMap::new();
        east.insert("region".to_string(), Value::s("us-east-2"));
        let mut west = IndexMap::new();
        west.insert("region".to_string(), Value::s("us-west-2"));
        arch.providers.push(ProviderRef {
            source: "hashicorp/aws".into(),
            name: "aws".into(),
            alias: None,
            config: east,
        });
        arch.providers.push(ProviderRef {
            source: "hashicorp/aws".into(),
            name: "aws".into(),
            alias: Some("west".into()),
            config: west,
        });
        let json = arch.render_terraform_json().unwrap();
        assert!(json["provider"]["aws"].is_array());
        let arr = json["provider"]["aws"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        // First entry was added first → us-east-2.
        assert_eq!(arr[0]["region"], "us-east-2");
        assert_eq!(arr[1]["region"], "us-west-2");
        assert_eq!(arr[1]["alias"], "west");
    }

    #[test]
    fn architecture_round_trips_through_serde() {
        // Critical: every typed value round-trips through serde so
        // .caixa.lisp consumers can ingest pre-rendered architectures
        // via :files capture without re-running render_terraform_json.
        let mut arch = Architecture::new("net");
        let mut a = IndexMap::new();
        a.insert("cidr_block".to_string(), Value::s("10.0.0.0/16"));
        arch.resources.push(Resource {
            type_id: "aws_vpc".to_string(),
            name: "main".to_string(),
            attributes: a,
            depends_on: vec![],
            provider: None,
            multiplicity: None,
        });
        let yaml = serde_yaml::to_string(&arch).unwrap();
        let parsed: Architecture = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(arch, parsed);
    }
}
