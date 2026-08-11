//! Typed dashboard document — the source side of the Grafana target.
//!
//! ## Why this is a fixed spine and not the recursive [`Value`](crate::Value)
//!
//! [`Value`](crate::Value) is the *authoring* tree: arbitrary nesting,
//! because a `.tlisp` author writes arbitrary parameter structures.
//! A dashboard *document* is not arbitrary. Measured against a real
//! 33-panel board (`kubernetes.json`, schemaVersion 39, 1601 lines): the
//! maximum nesting depth is 8, six of the seven container levels are a
//! fixed spine, and **every one of the 52 deepest nodes is a threshold
//! step field** — `panels[].fieldConfig.defaults.thresholds.steps[].*`.
//! Nothing in a Grafana dashboard is recursive. So the model is plain
//! nested structs: no boxing, no `serde_json::Value` outside the
//! deliberate per-target escape hatch.
//!
//! ## Byte-stability
//!
//! Serde emits struct fields in declaration order, so the wire shape is
//! owned by the type definitions in [`render`](crate::dashboard::render)
//! and rendering is a pure function of `(Dashboard, Theme)`. The one
//! rule that keeps it true: **no `HashMap` may enter the marshal path.**
//!
//! ## What lives here and what lives in a renderer
//!
//! The Datadog renderer is the oracle for that split — anything it
//! ignores or transforms is renderer-local, not model. So `gridPos`, the
//! whole `fieldConfig` envelope, panel ids, `refId` and the `options`
//! block are all computed at render time and appear nowhere below.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub mod render;

/// Semantic colour role.
///
/// A [`Threshold`] carries one of these, never a hex string and never a
/// Grafana colour name. That makes a hand-authored colour
/// **unrepresentable at the authoring layer** rather than merely
/// discouraged — the [`Theme`] decides the actual value at render time,
/// so every fleet dashboard reads the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Neutral,
    Primary,
    Info,
    Success,
    Warning,
    Danger,
}

/// Resolves a [`Role`] to a concrete colour.
///
/// `roles` is deliberately private and never serialized — it is read
/// only through [`Theme::color`], which keeps it out of the marshal path
/// and therefore out of the byte-stability question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub name: String,
    roles: BTreeMap<Role, String>,
}

impl Theme {
    /// Never panics: an unmapped role falls back to neutral.
    #[must_use]
    pub fn color(&self, r: Role) -> &str {
        self.roles
            .get(&r)
            .or_else(|| self.roles.get(&Role::Neutral))
            .map_or("#4C566A", String::as_str)
    }

    /// The fleet default — Nord's Polar Night / Frost / Aurora ramp.
    #[must_use]
    pub fn tundra() -> Self {
        let mut roles = BTreeMap::new();
        roles.insert(Role::Neutral, "#4C566A".to_string()); // nord3  Polar Night
        roles.insert(Role::Primary, "#88C0D0".to_string()); // nord8  Frost
        roles.insert(Role::Info, "#8FBCBB".to_string()); // nord7  Frost cyan
        roles.insert(Role::Success, "#A3BE8C".to_string()); // nord14 Aurora green
        roles.insert(Role::Warning, "#EBCB8B".to_string()); // nord13 Aurora yellow
        roles.insert(Role::Danger, "#BF616A".to_string()); // nord11 Aurora red
        Self {
            name: "tundra".to_string(),
            roles,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::tundra()
    }
}

/// The query language a datasource speaks.
///
/// This exists to make one specific production incident unrepresentable:
/// a logs panel pointed at a metrics datasource renders as an error in
/// the browser, not as a build failure. Carrying the language lets
/// [`Dashboard::validate`] reject the mismatch before anything ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryLang {
    PromQl,
    LogsQl,
    Sql,
}

/// A registered datasource.
///
/// Panels reference a datasource by `uid`, and the renderer emits the
/// `{type, uid}` object form schemaVersion 39 expects. A bare string
/// datasource is the pre-Grafana-8.3 shape and is not produced here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Datasource {
    pub uid: String,
    /// Grafana plugin type, e.g. `prometheus`.
    pub wire_type: String,
    pub query_lang: QueryLang,
}

impl Datasource {
    #[must_use]
    pub fn new(uid: impl Into<String>, wire_type: impl Into<String>, lang: QueryLang) -> Self {
        Self {
            uid: uid.into(),
            wire_type: wire_type.into(),
            query_lang: lang,
        }
    }
}

/// Ordered datasource registry. `Vec`, not a map, so it never reaches
/// the marshal path as an unordered container.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Datasources(pub Vec<Datasource>);

impl Datasources {
    #[must_use]
    pub fn get(&self, uid: &str) -> Option<&Datasource> {
        self.0.iter().find(|d| d.uid == uid)
    }
    pub fn register(&mut self, d: Datasource) {
        if let Some(slot) = self.0.iter_mut().find(|x| x.uid == d.uid) {
            *slot = d;
        } else {
            self.0.push(d);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelKind {
    Stat,
    TimeSeries,
    Gauge,
    Table,
    Heatmap,
    Text,
    Pie,
}

impl PanelKind {
    /// Uniform per-role sizes so rows align — alignment is the cheapest
    /// legibility win a dashboard has.
    #[must_use]
    pub fn default_width(self) -> u16 {
        match self {
            Self::Stat | Self::Gauge => 6,
            Self::Table | Self::Heatmap => 24,
            Self::Text => 8,
            Self::TimeSeries | Self::Pie => 12,
        }
    }
    #[must_use]
    pub fn default_height(self) -> u16 {
        match self {
            Self::Stat | Self::Gauge => 4,
            Self::Table | Self::Heatmap => 9,
            Self::Text => 3,
            Self::TimeSeries | Self::Pie => 8,
        }
    }
}

/// Whether a series is expected to be continuously present.
///
/// Not a rendering concern at all — it is what lets a health probe tell
/// *"wired but idle"* from *"never emitted"*. `up == 1` with the series
/// absent is a broken scrape, and without this field nothing can say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    #[default]
    Continuous,
    EventDriven,
    Conditional,
}

/// One band of a threshold ramp.
///
/// `value: None` is the base band. Grafana renders it as a literal
/// `null`, which is why this is `Option<f64>` rather than a sentinel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Threshold {
    pub value: Option<f64>,
    pub color: Role,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThresholdMode {
    #[default]
    Absolute,
    Percentage,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ThresholdConfig {
    pub mode: ThresholdMode,
    pub steps: Vec<Threshold>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Query {
    /// Grafana `refId`. Carried in the model rather than derived so two
    /// renderers agree on it.
    pub reference: String,
    pub expr: String,
    pub datasource_uid: String,
    pub legend: Option<String>,
    pub instant: bool,
    pub hide: bool,
    pub presence: Presence,
    /// Per-target expression overrides, keyed by target name.
    ///
    /// A PromQL expression is not portable to Datadog, and the honest
    /// response is to make the author supply the second rendition rather
    /// than guess. A renderer that finds neither a native expression nor
    /// an override for its target must fail loudly.
    pub alt_exprs: BTreeMap<String, String>,
}

impl Query {
    #[must_use]
    pub fn new(
        reference: impl Into<String>,
        expr: impl Into<String>,
        datasource_uid: impl Into<String>,
    ) -> Self {
        Self {
            reference: reference.into(),
            expr: expr.into(),
            datasource_uid: datasource_uid.into(),
            legend: None,
            instant: false,
            hide: false,
            presence: Presence::Continuous,
            alt_exprs: BTreeMap::new(),
        }
    }
    #[must_use]
    pub fn legend(mut self, l: impl Into<String>) -> Self {
        self.legend = Some(l.into());
        self
    }
    #[must_use]
    pub fn instant(mut self) -> Self {
        self.instant = true;
        self
    }
    #[must_use]
    pub fn presence(mut self, p: Presence) -> Self {
        self.presence = p;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    #[default]
    Auto,
    None,
    Value,
    Background,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphMode {
    #[default]
    Auto,
    None,
    Area,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Panel {
    /// Stable author-chosen identity. The integer Grafana id is
    /// positional and assigned at render time; this one survives a panel
    /// being inserted above it.
    pub id: String,
    pub kind: PanelKind,
    pub title: String,
    pub description: Option<String>,
    pub unit: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub decimals: Option<u32>,
    pub queries: Vec<Query>,
    pub thresholds: ThresholdConfig,
    pub width: u16,
    pub height: u16,
    pub display_mode: DisplayMode,
    pub graph: GraphMode,
}

impl Panel {
    /// A panel with no query renders as an empty box, so the constructor
    /// takes the queries rather than letting them be forgotten.
    #[must_use]
    pub fn new(id: impl Into<String>, kind: PanelKind, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind,
            title: title.into(),
            description: None,
            unit: None,
            min: None,
            max: None,
            decimals: None,
            queries: Vec::new(),
            thresholds: ThresholdConfig::default(),
            width: kind.default_width(),
            height: kind.default_height(),
            display_mode: DisplayMode::Auto,
            graph: GraphMode::Auto,
        }
    }
    #[must_use]
    pub fn query(mut self, q: Query) -> Self {
        self.queries.push(q);
        self
    }
    #[must_use]
    pub fn unit(mut self, u: impl Into<String>) -> Self {
        self.unit = Some(u.into());
        self
    }
    #[must_use]
    pub fn describe(mut self, d: impl Into<String>) -> Self {
        self.description = Some(d.into());
        self
    }
    #[must_use]
    pub fn thresholds(mut self, steps: Vec<Threshold>) -> Self {
        self.thresholds = ThresholdConfig {
            mode: ThresholdMode::Absolute,
            steps,
        };
        self
    }
    #[must_use]
    pub fn size(mut self, w: u16, h: u16) -> Self {
        self.width = w;
        self.height = h;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub title: String,
    pub collapsed: bool,
    pub panels: Vec<Panel>,
}

impl Row {
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            collapsed: false,
            panels: Vec::new(),
        }
    }
    #[must_use]
    pub fn panel(mut self, p: Panel) -> Self {
        self.panels.push(p);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeRange {
    pub from: String,
    pub to: String,
}

impl Default for TimeRange {
    fn default() -> Self {
        Self {
            from: "now-1h".to_string(),
            to: "now".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariableKind {
    Query,
    Constant,
    Custom,
    Datasource,
    Textbox,
    Interval,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    pub kind: VariableKind,
    pub label: Option<String>,
    pub datasource_uid: Option<String>,
    pub query: Option<String>,
    pub options: Vec<String>,
    pub multi: bool,
    pub include_all: bool,
}

impl Variable {
    #[must_use]
    pub fn new(name: impl Into<String>, kind: VariableKind) -> Self {
        Self {
            name: name.into(),
            kind,
            label: None,
            datasource_uid: None,
            query: None,
            options: Vec::new(),
            multi: false,
            include_all: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Annotation {
    pub name: String,
    pub datasource_uid: String,
    pub expr: String,
    pub color: Role,
    pub enable: bool,
}

/// A dashboard document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dashboard {
    pub uid: String,
    pub title: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    /// Auto-refresh interval, e.g. `30s`.
    ///
    /// Modelled *and emitted*. The Ruby renderer this model supersedes
    /// carried a `refresh` attribute through its DSL and its composition
    /// algebra and then never wrote it to the wire, so every dashboard it
    /// generated silently lost its refresh interval.
    pub refresh: Option<String>,
    pub time: TimeRange,
    pub timezone: String,
    pub editable: bool,
    pub variables: Vec<Variable>,
    pub annotations: Vec<Annotation>,
    pub rows: Vec<Row>,
    pub datasources: Datasources,
}

impl Dashboard {
    #[must_use]
    pub fn new(uid: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            uid: uid.into(),
            title: title.into(),
            description: None,
            tags: Vec::new(),
            refresh: Some("30s".to_string()),
            time: TimeRange::default(),
            timezone: "utc".to_string(),
            editable: true,
            variables: Vec::new(),
            annotations: Vec::new(),
            rows: Vec::new(),
            datasources: Datasources::default(),
        }
    }
    #[must_use]
    pub fn tag(mut self, t: impl Into<String>) -> Self {
        self.tags.push(t.into());
        self
    }
    #[must_use]
    pub fn row(mut self, r: Row) -> Self {
        self.rows.push(r);
        self
    }
    #[must_use]
    pub fn datasource(mut self, d: Datasource) -> Self {
        self.datasources.register(d);
        self
    }

    /// Reject the shapes that render as a silently-broken board.
    ///
    /// Each of these fails in the browser rather than at build time if
    /// left alone, which is the whole reason to check them here.
    pub fn validate(&self) -> Result<(), DashboardError> {
        if self.uid.is_empty() {
            return Err(DashboardError::MissingUid);
        }
        if self.title.is_empty() {
            return Err(DashboardError::MissingTitle {
                uid: self.uid.clone(),
            });
        }
        for row in &self.rows {
            for panel in &row.panels {
                if panel.title.is_empty() {
                    return Err(DashboardError::MissingPanelTitle {
                        panel: panel.id.clone(),
                    });
                }
                if panel.queries.is_empty() {
                    return Err(DashboardError::PanelWithoutQuery {
                        panel: panel.id.clone(),
                    });
                }
                for q in &panel.queries {
                    let Some(ds) = self.datasources.get(&q.datasource_uid) else {
                        return Err(DashboardError::UnknownDatasource {
                            panel: panel.id.clone(),
                            uid: q.datasource_uid.clone(),
                        });
                    };
                    if let Some(found) = classify(&q.expr) {
                        if found != ds.query_lang {
                            return Err(DashboardError::QueryLanguageMismatch {
                                panel: panel.id.clone(),
                                uid: ds.uid.clone(),
                                declared: ds.query_lang,
                                found,
                            });
                        }
                    }
                }
                if panel.width == 0 || panel.width > render::GRID {
                    return Err(DashboardError::PanelWidth {
                        panel: panel.id.clone(),
                        width: panel.width,
                    });
                }
            }
        }
        Ok(())
    }
}

/// Best-effort language classification.
///
/// Returns `None` when the expression carries no distinguishing marker —
/// a bare metric name is valid in more than one language, and guessing
/// there would produce false rejections. Only a positive identification
/// is reported.
///
/// String literals are blanked first: a `|` inside `status=~"a|b"` is
/// PromQL regex alternation, not a LogsQL pipe, and classifying without
/// stripping them mis-reads ordinary Prometheus selectors as logs
/// queries.
#[must_use]
fn classify(expr: &str) -> Option<QueryLang> {
    let bare = blank_string_literals(expr);
    let lower = bare.to_ascii_lowercase();
    if lower.contains("select ") && lower.contains(" from ") {
        return Some(QueryLang::Sql);
    }
    for f in [
        "rate(", "irate(", "increase(", "histogram_quantile(", "sum by", "avg by", "topk(",
        "absent(", "delta(", "predict_linear(",
    ] {
        if lower.contains(f) {
            return Some(QueryLang::PromQl);
        }
    }
    if bare.contains('|') {
        return Some(QueryLang::LogsQl);
    }
    None
}

fn blank_string_literals(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_str = false;
    for c in s.chars() {
        if c == '"' {
            in_str = !in_str;
            out.push('"');
        } else if in_str {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DashboardError {
    #[error("dashboard has no uid")]
    MissingUid,
    #[error("dashboard {uid} has no title")]
    MissingTitle { uid: String },
    #[error("panel {panel} has no title")]
    MissingPanelTitle { panel: String },
    #[error("panel {panel} has no query — it would render as an empty box")]
    PanelWithoutQuery { panel: String },
    #[error("panel {panel} references unregistered datasource {uid}")]
    UnknownDatasource { panel: String, uid: String },
    #[error(
        "panel {panel}: datasource {uid} speaks {declared:?} but the query looks like {found:?}"
    )]
    QueryLanguageMismatch {
        panel: String,
        uid: String,
        declared: QueryLang,
        found: QueryLang,
    },
    #[error("panel {panel}: width {width} is outside 1..=24")]
    PanelWidth { panel: String, width: u16 },
}
