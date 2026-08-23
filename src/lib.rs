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

pub mod dashboard;
pub use dashboard::{
    Annotation, Dashboard, DashboardError, Datasource, Datasources, DisplayMode, GraphMode, Panel,
    PanelKind, Presence, Query, QueryLang, Role, Row, Theme, Threshold, ThresholdConfig,
    ThresholdMode, TimeRange, Variable, VariableKind,
};

pub mod synthesizer;
pub use synthesizer::{
    CrossplaneYaml, GrafanaJson, MagmaPlan, RenderTarget, Synthesizer, TerraformJson,
};

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

/// A typed document value.
///
/// Genuinely recursive: a [`ResourceRef`] survives at any depth, inside
/// lists and maps alike. The previous shape — `Ref(ResourceRef) |
/// Json(serde_json::Value)` — delegated all structure to serde_json and
/// so could only carry a reference at the top level of an attribute:
/// `Value::arr` projected every item through `into_json` at construction
/// time, stringifying nested references before anyone could walk them.
/// The doc comment on [`ResourceRef`] promised a dep graph walkable
/// "without parsing interpolation strings"; below depth 0 that promise
/// was false. It is now true at every depth.
///
/// This is the one document tree for the lava suite. `lava-chart`'s
/// private `ValueTree` is the same shape and is retired in favour of it;
/// note the two are *not* interchangeable by name — that crate's
/// `Ref { paths }` is a Helm `.Values.a.b` lookup, semantically
/// unrelated to a `ResourceRef` resource-graph coordinate.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Ref(ResourceRef),
    List(Vec<Value>),
    Map(IndexMap<String, Value>),
}

impl Value {
    #[must_use]
    pub fn s(v: impl Into<String>) -> Self {
        Self::Str(v.into())
    }
    #[must_use]
    pub fn b(v: bool) -> Self {
        Self::Bool(v)
    }
    #[must_use]
    pub fn n(v: i64) -> Self {
        Self::Int(v)
    }
    #[must_use]
    pub fn f(v: f64) -> Self {
        Self::Float(v)
    }
    /// Build a list. Unlike the previous implementation this does **not**
    /// project items through `into_json` — a `Ref` inside a list stays a
    /// `Ref`.
    #[must_use]
    pub fn arr(items: impl IntoIterator<Item = Value>) -> Self {
        Self::List(items.into_iter().collect())
    }
    #[must_use]
    pub fn map(entries: impl IntoIterator<Item = (String, Value)>) -> Self {
        Self::Map(entries.into_iter().collect())
    }

    /// Project to JSON for emission. References render as
    /// `${type.name.attribute}` per Terraform's interpolation syntax, at
    /// whatever depth they sit.
    ///
    /// Map ordering is insertion order, not alphabetical — `serde_json`
    /// is built here with `preserve_order` precisely so this projection
    /// cannot quietly re-sort and falsify the byte-stability guarantee
    /// [`Architecture::render_terraform_json`] makes.
    #[must_use]
    pub fn into_json(self) -> serde_json::Value {
        match self {
            Self::Null => serde_json::Value::Null,
            Self::Bool(b) => serde_json::Value::Bool(b),
            Self::Int(n) => serde_json::Value::Number(n.into()),
            Self::Float(f) => serde_json::Number::from_f64(f)
                .map_or(serde_json::Value::Null, serde_json::Value::Number),
            Self::Str(s) => serde_json::Value::String(s),
            Self::Ref(r) => serde_json::Value::String(r.to_interpolation()),
            Self::List(items) => {
                serde_json::Value::Array(items.into_iter().map(Value::into_json).collect())
            }
            Self::Map(entries) => serde_json::Value::Object(
                entries
                    .into_iter()
                    .map(|(k, v)| (k, v.into_json()))
                    .collect(),
            ),
        }
    }

    /// Lift a JSON document back into the tree. A string of the exact
    /// shape `${a.b.c}` becomes a [`Value::Ref`]; every other string
    /// stays a [`Value::Str`]. See [`ResourceRef::from_interpolation`].
    #[must_use]
    pub fn from_json(v: serde_json::Value) -> Self {
        match v {
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Bool(b) => Self::Bool(b),
            serde_json::Value::Number(n) => n
                .as_i64()
                .map_or_else(|| Self::Float(n.as_f64().unwrap_or(0.0)), Self::Int),
            serde_json::Value::String(s) => {
                ResourceRef::from_interpolation(&s).map_or(Self::Str(s), Self::Ref)
            }
            serde_json::Value::Array(items) => {
                Self::List(items.into_iter().map(Value::from_json).collect())
            }
            serde_json::Value::Object(entries) => Self::Map(
                entries
                    .into_iter()
                    .map(|(k, v)| (k, Value::from_json(v)))
                    .collect(),
            ),
        }
    }
}

/// Serde goes through the JSON projection, so the serialized form and the
/// rendered form are the same bytes.
///
/// This also removes a real ambiguity the derived `#[serde(untagged)]`
/// impl carried: `Ref` was a bare `ResourceRef`, so *any* three-key
/// object named `{type_id, name, attribute}` deserialized back as a
/// reference rather than as the map it was written as. Untagged tries
/// variants in declaration order and cannot be told otherwise. Routing
/// through `${…}` makes the discrimination syntactic and total.
impl Serialize for Value {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        self.clone().into_json().serialize(ser)
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        Ok(Self::from_json(serde_json::Value::deserialize(de)?))
    }
}

impl ResourceRef {
    /// Render as Terraform's interpolation syntax: `${type.name.attribute}`.
    #[must_use]
    pub fn to_interpolation(&self) -> String {
        let mut s = String::with_capacity(
            self.type_id.len() + self.name.len() + self.attribute.len() + 5,
        );
        s.push_str("${");
        s.push_str(&self.type_id);
        s.push('.');
        s.push_str(&self.name);
        s.push('.');
        s.push_str(&self.attribute);
        s.push('}');
        s
    }

    /// Parse the exact shape `${a.b.c}` back into a reference.
    ///
    /// Deliberately strict: the whole string must be one interpolation
    /// with exactly three dot-separated segments and nothing outside the
    /// braces. `"prefix-${a.b.c}"`, `"${var.foo}"` and `"${a.b.c.d}"` all
    /// return `None` and stay strings, because none of them is a
    /// resource-output coordinate.
    #[must_use]
    pub fn from_interpolation(s: &str) -> Option<Self> {
        let inner = s.strip_prefix("${")?.strip_suffix('}')?;
        if inner.contains(['$', '{', '}']) {
            return None;
        }
        let mut parts = inner.split('.');
        let type_id = parts.next()?;
        let name = parts.next()?;
        let attribute = parts.next()?;
        if parts.next().is_some()
            || type_id.is_empty()
            || name.is_empty()
            || attribute.is_empty()
        {
            return None;
        }
        Some(Self {
            type_id: type_id.to_string(),
            name: name.to_string(),
            attribute: attribute.to_string(),
        })
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
    /// `import` blocks — terraform `import { to = …, id = … }`.
    ///
    /// ── ★ WHY A RENDERER NEEDS THESE AT ALL ──────────────────────────
    /// Without them, an architecture describing infrastructure that ALREADY
    /// EXISTS plans to CREATE it. For a catalogue of ~1000 repositories that
    /// is a thousand `422 name already exists` failures, and the plan looks
    /// entirely reasonable right up until apply.
    ///
    /// Adopt-not-create is therefore not a convenience: it is the difference
    /// between a renderer that can describe a live estate and one that can
    /// only describe a greenfield.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imports: Vec<Import>,
}

/// One terraform `import` block: adopt the existing object `id` into the
/// address `to`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Import {
    /// Resource address, e.g. `github_repository.foo`.
    pub to: String,
    /// Provider-specific id of the existing object.
    pub id: String,
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
            imports: Vec::new(),
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

        // import blocks
        //
        // An ARRAY, because terraform's import block is repeatable and each
        // entry is a separate adoption — unlike `resource`/`data`, which nest
        // by type and name. Rendering it as an object keyed by address would
        // parse and then import nothing.
        if !self.imports.is_empty() {
            let arr: Vec<serde_json::Value> = self
                .imports
                .iter()
                .map(|i| {
                    let mut m = serde_json::Map::new();
                    m.insert("to".to_string(), serde_json::Value::String(i.to.clone()));
                    m.insert("id".to_string(), serde_json::Value::String(i.id.clone()));
                    serde_json::Value::Object(m)
                })
                .collect();
            root.insert("import".to_string(), serde_json::Value::Array(arr));
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
    /// The source document was rejected before rendering. Carries the
    /// originating error's message so a `Synthesizer` caller sees why
    /// without having to know which document type produced it.
    #[error("{0}")]
    Invalid(String),
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

    fn vpc_id_ref() -> ResourceRef {
        ResourceRef {
            type_id: "aws_vpc".into(),
            name: "main".into(),
            attribute: "id".into(),
        }
    }

    // ── the depth defect, both directions ────────────────────────────
    //
    // These are the red-run for the reason `Value` was replaced. Against
    // the previous `Ref(ResourceRef) | Json(serde_json::Value)` shape both
    // of them FAIL: `Value::arr` projected each item through `into_json`
    // at construction, so the reference was already a `${…}` string before
    // the assertion ran, and a map could only ever hold `serde_json`
    // values. The dep-graph promise on `ResourceRef` held at depth 0 only.

    #[test]
    fn a_ref_survives_typed_inside_a_list() {
        let v = Value::arr([Value::s("literal"), Value::Ref(vpc_id_ref())]);
        let Value::List(items) = &v else {
            panic!("expected a list, got {v:?}");
        };
        assert_eq!(items.len(), 2);
        assert!(
            matches!(&items[1], Value::Ref(r) if r == &vpc_id_ref()),
            "the reference was flattened to {:?} instead of staying typed",
            items[1]
        );
        // …and still renders correctly once projected.
        assert_eq!(v.into_json()[1], "${aws_vpc.main.id}");
    }

    #[test]
    fn a_ref_survives_typed_inside_a_map_at_depth() {
        let inner = Value::map([("vpc_id".to_string(), Value::Ref(vpc_id_ref()))]);
        let outer = Value::map([("network".to_string(), inner)]);
        let Value::Map(top) = &outer else {
            panic!("expected a map, got {outer:?}");
        };
        let Some(Value::Map(nested)) = top.get("network") else {
            panic!("expected a nested map");
        };
        assert!(matches!(nested.get("vpc_id"), Some(Value::Ref(r)) if r == &vpc_id_ref()));
        assert_eq!(
            outer.into_json()["network"]["vpc_id"],
            "${aws_vpc.main.id}"
        );
    }

    // ── byte-stability ───────────────────────────────────────────────

    #[test]
    fn map_keys_render_in_insertion_order_not_alphabetically() {
        // render_terraform_json documents itself as byte-stable. Before
        // `preserve_order`, `into_json` built a BTreeMap-backed
        // serde_json::Map and silently re-sorted every map, so this
        // rendered as {"Alpha":…,"Name":…,"Zulu":…}.
        let tags = Value::map([
            ("Zulu".to_string(), Value::s("last-alphabetically")),
            ("Alpha".to_string(), Value::s("first-alphabetically")),
            ("Name".to_string(), Value::s("middle")),
        ]);
        let rendered = serde_json::to_string(&tags.into_json()).unwrap();
        assert_eq!(
            rendered,
            r#"{"Zulu":"last-alphabetically","Alpha":"first-alphabetically","Name":"middle"}"#
        );
    }

    #[test]
    fn rendering_is_byte_identical_across_repeated_calls() {
        let v = Value::map([
            ("b".to_string(), Value::arr([Value::n(2), Value::n(1)])),
            ("a".to_string(), Value::Ref(vpc_id_ref())),
        ]);
        let first = serde_json::to_string(&v.clone().into_json()).unwrap();
        for _ in 0..16 {
            assert_eq!(
                serde_json::to_string(&v.clone().into_json()).unwrap(),
                first
            );
        }
    }

    // ── the untagged-Ref ambiguity ───────────────────────────────────

    #[test]
    fn interpolation_round_trips_through_serde() {
        let v = Value::Ref(vpc_id_ref());
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, r#""${aws_vpc.main.id}""#);
        let back: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn a_three_key_map_is_not_mistaken_for_a_reference() {
        // The derived #[serde(untagged)] impl deserialized ANY object
        // carrying {type_id, name, attribute} back as a Ref, because
        // untagged tries variants in declaration order and cannot be told
        // otherwise. Routing serde through `${…}` makes the
        // discrimination syntactic, so a map that merely happens to use
        // those key names stays a map.
        let map = Value::map([
            ("type_id".to_string(), Value::s("aws_vpc")),
            ("name".to_string(), Value::s("main")),
            ("attribute".to_string(), Value::s("id")),
        ]);
        let round_tripped: Value =
            serde_json::from_str(&serde_json::to_string(&map).unwrap()).unwrap();
        assert_eq!(round_tripped, map, "a plain map came back as {round_tripped:?}");
        assert!(matches!(round_tripped, Value::Map(_)));
    }

    #[test]
    fn only_an_exact_three_segment_interpolation_parses_as_a_reference() {
        assert_eq!(
            ResourceRef::from_interpolation("${aws_vpc.main.id}"),
            Some(vpc_id_ref())
        );
        // Everything below is a string, not a resource-output coordinate.
        for not_a_ref in [
            "${var.foo}",             // two segments — an input variable
            "${aws_vpc.main.id.sub}", // four segments
            "prefix-${aws_vpc.main.id}", // interpolation is not the whole string
            "${aws_vpc.main.id}-suffix",
            "${}",
            "${..}",
            "${aws_vpc..id}", // empty middle segment
            "aws_vpc.main.id", // no braces at all
            "${a.b.c}${d.e.f}", // two interpolations
        ] {
            assert_eq!(
                ResourceRef::from_interpolation(not_a_ref),
                None,
                "{not_a_ref:?} should not parse as a reference"
            );
            // …and survives a serde round-trip as a plain string.
            let v = Value::s(not_a_ref);
            let back: Value =
                serde_json::from_str(&serde_json::to_string(&v).unwrap()).unwrap();
            assert_eq!(back, v);
        }
    }
}

#[cfg(test)]
mod import_block_tests {
    use super::*;

    #[test]
    fn imports_render_as_a_top_level_array() {
        let mut arch = Architecture::new("adopt");
        arch.imports.push(Import { to: "github_repository.alpha".into(), id: "alpha".into() });
        arch.imports.push(Import { to: "github_repository.beta".into(), id: "beta".into() });
        let json = arch.render_terraform_json().unwrap();
        // ★ ARRAY, not an object keyed by address. terraform's import block
        // is repeatable; an object would parse and import nothing.
        assert!(json["import"].is_array(), "got {}", json["import"]);
        assert_eq!(json["import"][0]["to"], "github_repository.alpha");
        assert_eq!(json["import"][0]["id"], "alpha");
        assert_eq!(json["import"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn no_imports_emits_no_import_key() {
        // An empty `import: []` is not the same document as no import block,
        // and a renderer that always emits one makes every greenfield plan
        // carry an adoption section it does not have.
        let arch = Architecture::new("greenfield");
        let json = arch.render_terraform_json().unwrap();
        assert!(json.get("import").is_none(), "got {json}");
    }
}
