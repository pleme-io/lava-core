//! Grafana JSON emission — schemaVersion 39.
//!
//! The `j*` types below mirror exactly the subset of the Grafana schema
//! this renderer emits, and they are **private on purpose**: the wire
//! shape is owned here, not by the authoring model. Serde emits struct
//! fields in declaration order, so with no map in the marshal path the
//! output is byte-stable by construction rather than by convention.
//!
//! Everything in this file is renderer-local by the Datadog test: if a
//! second target would ignore or transform it, it does not belong in the
//! model. That covers `gridPos` and the whole layout computation, the
//! integer panel ids, `refId`, the `fieldConfig` envelope, the per-kind
//! `options` block, and the kind→type name mapping.

use serde::Serialize;

use super::{
    Dashboard, DashboardError, DisplayMode, GraphMode, PanelKind, QueryLang, Theme, ThresholdMode,
    VariableKind,
};

/// Grafana's fixed 24-column grid.
pub const GRID: u16 = 24;

/// The schema version this renderer targets.
pub const SCHEMA_VERSION: u32 = 39;

#[derive(Serialize)]
struct JDashboard {
    annotations: JAnnotations,
    description: String,
    editable: bool,
    #[serde(rename = "fiscalYearStartMonth")]
    fiscal_year_start_month: u32,
    #[serde(rename = "graphTooltip")]
    graph_tooltip: u32,
    id: Option<u32>,
    links: Vec<()>,
    panels: Vec<JPanel>,
    /// Emitted. The Ruby renderer modelled this and dropped it on the
    /// floor, so every board it produced lost its refresh interval.
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh: Option<String>,
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    tags: Vec<String>,
    templating: JTemplating,
    time: JTime,
    timepicker: JTimepicker,
    timezone: String,
    title: String,
    uid: String,
    version: u32,
}

#[derive(Serialize)]
struct JAnnotations {
    list: Vec<JAnnotation>,
}

#[derive(Serialize)]
struct JAnnotation {
    name: String,
    datasource: JDatasourceRef,
    expr: String,
    #[serde(rename = "iconColor")]
    icon_color: String,
    enable: bool,
}

#[derive(Serialize)]
struct JTemplating {
    list: Vec<JVariable>,
}

#[derive(Serialize)]
struct JVariable {
    name: String,
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    datasource: Option<JDatasourceRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    query: Option<String>,
    options: Vec<JVarOption>,
    multi: bool,
    #[serde(rename = "includeAll")]
    include_all: bool,
    refresh: u32,
}

#[derive(Serialize)]
struct JVarOption {
    value: String,
    text: String,
    selected: bool,
}

#[derive(Serialize)]
struct JTime {
    from: String,
    to: String,
}

#[derive(Serialize)]
struct JTimepicker {}

#[derive(Serialize, Clone)]
struct JDatasourceRef {
    #[serde(rename = "type")]
    wire_type: String,
    uid: String,
}

#[derive(Serialize, Clone)]
struct JGridPos {
    h: u16,
    w: u16,
    x: u16,
    y: u16,
}

#[derive(Serialize, Clone)]
struct JPanel {
    id: u32,
    title: String,
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(rename = "gridPos")]
    grid_pos: JGridPos,
    #[serde(skip_serializing_if = "Option::is_none")]
    collapsed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    targets: Option<Vec<JTarget>>,
    #[serde(rename = "fieldConfig", skip_serializing_if = "Option::is_none")]
    field_config: Option<JFieldConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<JOptions>,
    /// A collapsed row carries its children HERE, not as following
    /// siblings. Grafana renders a collapsed row whose `panels` is empty
    /// as an empty row with its children orphaned below it — the exact
    /// bug the Ruby renderer shipped, latent only because every board in
    /// production happened to use `collapsed: false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    panels: Option<Vec<JPanel>>,
}

#[derive(Serialize, Clone)]
struct JTarget {
    #[serde(rename = "refId")]
    ref_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    expr: Option<String>,
    #[serde(rename = "rawSql", skip_serializing_if = "Option::is_none")]
    raw_sql: Option<String>,
    #[serde(rename = "queryType", skip_serializing_if = "Option::is_none")]
    query_type: Option<&'static str>,
    #[serde(rename = "editorType", skip_serializing_if = "Option::is_none")]
    editor_type: Option<&'static str>,
    datasource: JDatasourceRef,
    #[serde(rename = "legendFormat", skip_serializing_if = "Option::is_none")]
    legend_format: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    instant: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    hide: bool,
}

#[derive(Serialize, Clone)]
struct JFieldConfig {
    defaults: JFieldDefaults,
    overrides: Vec<()>,
}

#[derive(Serialize, Clone)]
struct JFieldDefaults {
    #[serde(skip_serializing_if = "Option::is_none")]
    unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    decimals: Option<u32>,
    color: JColor,
    #[serde(skip_serializing_if = "Option::is_none")]
    thresholds: Option<JThresholds>,
}

#[derive(Serialize, Clone)]
struct JColor {
    mode: &'static str,
}

#[derive(Serialize, Clone)]
struct JThresholds {
    mode: &'static str,
    steps: Vec<JThreshold>,
}

#[derive(Serialize, Clone)]
struct JThreshold {
    color: String,
    value: Option<f64>,
}

#[derive(Serialize, Clone)]
struct JOptions {
    #[serde(rename = "reduceOptions", skip_serializing_if = "Option::is_none")]
    reduce_options: Option<JReduceOptions>,
    #[serde(rename = "colorMode", skip_serializing_if = "Option::is_none")]
    color_mode: Option<&'static str>,
    #[serde(rename = "graphMode", skip_serializing_if = "Option::is_none")]
    graph_mode: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    legend: Option<JLegend>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tooltip: Option<JTooltip>,
}

#[derive(Serialize, Clone)]
struct JReduceOptions {
    calcs: Vec<&'static str>,
    fields: &'static str,
    values: bool,
}

#[derive(Serialize, Clone)]
struct JLegend {
    #[serde(rename = "displayMode")]
    display_mode: &'static str,
    placement: &'static str,
    calcs: Vec<&'static str>,
}

#[derive(Serialize, Clone)]
struct JTooltip {
    mode: &'static str,
    sort: &'static str,
}

fn kind_name(k: PanelKind) -> &'static str {
    match k {
        PanelKind::Stat => "stat",
        PanelKind::TimeSeries => "timeseries",
        PanelKind::Gauge => "gauge",
        PanelKind::Table => "table",
        PanelKind::Heatmap => "heatmap",
        PanelKind::Text => "text",
        // Grafana's plugin id is `piechart`; Datadog's equivalent is
        // `sunburst`. Kind mapping is per-renderer, always.
        PanelKind::Pie => "piechart",
    }
}

fn variable_kind_name(k: VariableKind) -> &'static str {
    match k {
        VariableKind::Query => "query",
        VariableKind::Constant => "constant",
        VariableKind::Custom => "custom",
        VariableKind::Datasource => "datasource",
        VariableKind::Textbox => "textbox",
        VariableKind::Interval => "interval",
    }
}

impl Dashboard {
    /// Render to Grafana dashboard JSON.
    ///
    /// Validates first: every shape [`Dashboard::validate`] rejects would
    /// otherwise render as a board that looks fine and is broken in the
    /// browser.
    pub fn render_grafana_json(
        &self,
        theme: &Theme,
    ) -> Result<serde_json::Value, DashboardError> {
        self.validate()?;

        let mut panels: Vec<JPanel> = Vec::new();
        let mut next_id: u32 = 1;
        let mut cursor_y: u16 = 0;

        for row in &self.rows {
            let children = self.render_row_panels(row, &mut next_id, cursor_y + 1, theme);
            let row_height: u16 = children
                .iter()
                .map(|p| p.grid_pos.y + p.grid_pos.h)
                .max()
                .unwrap_or(cursor_y + 1)
                .saturating_sub(cursor_y + 1);

            panels.push(JPanel {
                id: next_id,
                title: row.title.clone(),
                kind: "row",
                grid_pos: JGridPos {
                    h: 1,
                    w: GRID,
                    x: 0,
                    y: cursor_y,
                },
                collapsed: Some(row.collapsed),
                description: None,
                targets: None,
                field_config: None,
                options: None,
                panels: if row.collapsed {
                    Some(children.clone())
                } else {
                    Some(Vec::new())
                },
            });
            next_id += 1;
            cursor_y += 1;

            if row.collapsed {
                // Children live inside the row object; nothing follows.
            } else {
                cursor_y += row_height;
                panels.extend(children);
            }
        }

        let out = JDashboard {
            annotations: JAnnotations {
                list: self
                    .annotations
                    .iter()
                    .map(|a| JAnnotation {
                        name: a.name.clone(),
                        datasource: self.dsref(&a.datasource_uid),
                        expr: a.expr.clone(),
                        icon_color: theme.color(a.color).to_string(),
                        enable: a.enable,
                    })
                    .collect(),
            },
            description: self.description.clone().unwrap_or_default(),
            editable: self.editable,
            fiscal_year_start_month: 0,
            graph_tooltip: 1,
            id: None,
            links: Vec::new(),
            panels,
            refresh: self.refresh.clone(),
            schema_version: SCHEMA_VERSION,
            tags: self.tags.clone(),
            templating: JTemplating {
                list: self
                    .variables
                    .iter()
                    .map(|v| JVariable {
                        name: v.name.clone(),
                        kind: variable_kind_name(v.kind),
                        label: v.label.clone(),
                        datasource: v.datasource_uid.as_ref().map(|u| self.dsref(u)),
                        query: v.query.clone(),
                        options: v
                            .options
                            .iter()
                            .enumerate()
                            .map(|(i, o)| JVarOption {
                                value: o.clone(),
                                text: o.clone(),
                                selected: i == 0,
                            })
                            .collect(),
                        multi: v.multi,
                        include_all: v.include_all,
                        refresh: u32::from(matches!(v.kind, VariableKind::Query)),
                    })
                    .collect(),
            },
            time: JTime {
                from: self.time.from.clone(),
                to: self.time.to.clone(),
            },
            timepicker: JTimepicker {},
            timezone: self.timezone.clone(),
            title: self.title.clone(),
            uid: self.uid.clone(),
            version: 1,
        };

        serde_json::to_value(out).map_err(|e| DashboardError::MissingTitle {
            uid: format!("serialize failed: {e}"),
        })
    }

    fn dsref(&self, uid: &str) -> JDatasourceRef {
        let wire_type = self
            .datasources
            .get(uid)
            .map_or("prometheus", |d| d.wire_type.as_str());
        JDatasourceRef {
            wire_type: wire_type.to_string(),
            uid: uid.to_string(),
        }
    }

    fn render_row_panels(
        &self,
        row: &super::Row,
        next_id: &mut u32,
        start_y: u16,
        theme: &Theme,
    ) -> Vec<JPanel> {
        let mut out = Vec::new();
        let mut x: u16 = 0;
        let mut y = start_y;
        let mut row_max_h: u16 = 0;

        for panel in &row.panels {
            if x + panel.width > GRID {
                y += row_max_h;
                x = 0;
                row_max_h = 0;
            }
            let is_sql = panel.queries.first().is_some_and(|q| {
                self.datasources
                    .get(&q.datasource_uid)
                    .is_some_and(|d| d.query_lang == QueryLang::Sql)
            });

            let targets: Vec<JTarget> = panel
                .queries
                .iter()
                .map(|q| {
                    let sql = self
                        .datasources
                        .get(&q.datasource_uid)
                        .is_some_and(|d| d.query_lang == QueryLang::Sql);
                    JTarget {
                        ref_id: q.reference.clone(),
                        // The ClickHouse plugin reads rawSql + queryType;
                        // an `expr` would be ignored and the panel would
                        // render empty. Two arms, not one.
                        expr: if sql { None } else { Some(q.expr.clone()) },
                        raw_sql: if sql { Some(q.expr.clone()) } else { None },
                        query_type: if sql { Some("table") } else { None },
                        editor_type: if sql { Some("sql") } else { None },
                        datasource: self.dsref(&q.datasource_uid),
                        legend_format: q.legend.clone(),
                        instant: q.instant,
                        hide: q.hide,
                    }
                })
                .collect();

            let has_thresholds = !panel.thresholds.steps.is_empty();
            let field_config = JFieldConfig {
                defaults: JFieldDefaults {
                    unit: panel.unit.clone(),
                    min: panel.min,
                    max: panel.max,
                    decimals: panel.decimals,
                    color: JColor {
                        mode: if has_thresholds {
                            "thresholds"
                        } else {
                            "palette-classic"
                        },
                    },
                    thresholds: has_thresholds.then(|| JThresholds {
                        mode: match panel.thresholds.mode {
                            ThresholdMode::Absolute => "absolute",
                            ThresholdMode::Percentage => "percentage",
                        },
                        steps: panel
                            .thresholds
                            .steps
                            .iter()
                            .enumerate()
                            .map(|(i, s)| JThreshold {
                                color: theme.color(s.color).to_string(),
                                // Grafana's first step is the base band
                                // and must carry a null value whatever the
                                // author wrote.
                                value: if i == 0 { None } else { s.value },
                            })
                            .collect(),
                    }),
                },
                overrides: Vec::new(),
            };

            out.push(JPanel {
                id: *next_id,
                title: panel.title.clone(),
                kind: kind_name(panel.kind),
                grid_pos: JGridPos {
                    h: panel.height,
                    w: panel.width,
                    x,
                    y,
                },
                collapsed: None,
                description: panel.description.clone(),
                targets: Some(targets),
                field_config: Some(field_config),
                options: options_for(panel, is_sql),
                panels: None,
            });
            *next_id += 1;
            x += panel.width;
            row_max_h = row_max_h.max(panel.height);
        }
        out
    }
}

fn options_for(panel: &super::Panel, _is_sql: bool) -> Option<JOptions> {
    match panel.kind {
        PanelKind::Stat => Some(JOptions {
            reduce_options: Some(JReduceOptions {
                calcs: vec!["lastNotNull"],
                fields: "",
                values: false,
            }),
            color_mode: Some(match panel.display_mode {
                DisplayMode::Background => "background",
                DisplayMode::None => "none",
                DisplayMode::Value | DisplayMode::Auto => "value",
            }),
            // A one-point sparkline is noise, so an all-instant stat
            // panel gets no graph.
            graph_mode: Some(match panel.graph {
                GraphMode::None => "none",
                GraphMode::Area => "area",
                GraphMode::Auto => {
                    if panel.queries.iter().all(|q| q.instant) {
                        "none"
                    } else {
                        "area"
                    }
                }
            }),
            legend: None,
            tooltip: None,
        }),
        PanelKind::Gauge => Some(JOptions {
            reduce_options: Some(JReduceOptions {
                calcs: vec!["lastNotNull"],
                fields: "",
                values: false,
            }),
            color_mode: None,
            graph_mode: None,
            legend: None,
            tooltip: None,
        }),
        PanelKind::TimeSeries => Some(JOptions {
            reduce_options: None,
            color_mode: None,
            graph_mode: None,
            legend: Some(JLegend {
                display_mode: "table",
                placement: "bottom",
                calcs: vec!["lastNotNull", "max", "mean"],
            }),
            tooltip: Some(JTooltip {
                mode: "multi",
                sort: "desc",
            }),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::dashboard::*;

    fn ds() -> Datasource {
        Datasource::new("mimir", "prometheus", QueryLang::PromQl)
    }

    fn board() -> Dashboard {
        Dashboard::new("fedramp-audit", "FedRAMP · audit pipeline")
            .tag("fedramp")
            .datasource(ds())
            .row(
                Row::new("audit sink").panel(
                    Panel::new("sink_errors", PanelKind::Stat, "Sink errors /5m")
                        .query(Query::new(
                            "A",
                            "sum(increase(vector_component_errors_total[5m]))",
                            "mimir",
                        ))
                        .thresholds(vec![
                            Threshold { value: None, color: Role::Success },
                            Threshold { value: Some(1.0), color: Role::Danger },
                        ]),
                ),
            )
    }

    #[test]
    fn renders_schema_39_with_the_object_datasource_form() {
        let j = board().render_grafana_json(&Theme::tundra()).unwrap();
        assert_eq!(j["schemaVersion"], 39);
        // A bare string datasource is the pre-8.3 shape. schemaVersion 39
        // wants {type, uid}; emitting the old form while declaring 39 is
        // the fidelity bug in the Go implementation this model replaces.
        let target_ds = &j["panels"][1]["targets"][0]["datasource"];
        assert_eq!(target_ds["type"], "prometheus");
        assert_eq!(target_ds["uid"], "mimir");
        assert!(!target_ds.is_string());
    }

    #[test]
    fn refresh_is_actually_emitted() {
        // The Ruby renderer modelled `refresh`, threaded it through its
        // composition algebra, and never wrote it — so every dashboard it
        // generated silently lost its refresh interval.
        let j = board().render_grafana_json(&Theme::tundra()).unwrap();
        assert_eq!(j["refresh"], "30s");
    }

    #[test]
    fn the_base_threshold_step_is_null_whatever_the_author_wrote() {
        let mut b = board();
        b.rows[0].panels[0].thresholds.steps[0].value = Some(999.0);
        let j = b.render_grafana_json(&Theme::tundra()).unwrap();
        let steps = &j["panels"][1]["fieldConfig"]["defaults"]["thresholds"]["steps"];
        assert!(steps[0]["value"].is_null(), "base step must be null");
        assert_eq!(steps[1]["value"], 1.0);
        // Colours come from the theme, never from the author.
        assert_eq!(steps[0]["color"], "#A3BE8C");
        assert_eq!(steps[1]["color"], "#BF616A");
    }

    #[test]
    fn a_collapsed_row_nests_its_children_instead_of_orphaning_them() {
        // Grafana requires a collapsed row's children inside row.panels.
        // The Ruby renderer always emitted `panels: []` and appended the
        // children as siblings, so a collapsed row rendered empty with its
        // panels stranded below it. Latent only because every production
        // board used collapsed: false.
        let mut b = board();
        b.rows[0].collapsed = true;
        let j = b.render_grafana_json(&Theme::tundra()).unwrap();
        let panels = j["panels"].as_array().unwrap();
        assert_eq!(panels.len(), 1, "the row must be the only top-level panel");
        assert_eq!(panels[0]["type"], "row");
        assert_eq!(panels[0]["collapsed"], true);
        assert_eq!(panels[0]["panels"].as_array().unwrap().len(), 1);
        assert_eq!(panels[0]["panels"][0]["title"], "Sink errors /5m");
    }

    #[test]
    fn an_expanded_row_leaves_its_children_as_siblings() {
        let j = board().render_grafana_json(&Theme::tundra()).unwrap();
        let panels = j["panels"].as_array().unwrap();
        assert_eq!(panels.len(), 2);
        assert_eq!(panels[0]["type"], "row");
        assert!(panels[0]["panels"].as_array().unwrap().is_empty());
        assert_eq!(panels[1]["title"], "Sink errors /5m");
    }

    #[test]
    fn panels_wrap_at_the_24_column_boundary() {
        let wide = |id: &str| {
            Panel::new(id, PanelKind::TimeSeries, id)
                .query(Query::new("A", "up", "mimir"))
                .size(12, 8)
        };
        let b = Dashboard::new("grid", "grid").datasource(ds()).row(
            Row::new("r").panel(wide("a")).panel(wide("b")).panel(wide("c")),
        );
        let j = b.render_grafana_json(&Theme::tundra()).unwrap();
        let p = j["panels"].as_array().unwrap();
        // index 0 is the row header at y=0; children start at y=1.
        assert_eq!((p[1]["gridPos"]["x"].as_u64(), p[1]["gridPos"]["y"].as_u64()), (Some(0), Some(1)));
        assert_eq!((p[2]["gridPos"]["x"].as_u64(), p[2]["gridPos"]["y"].as_u64()), (Some(12), Some(1)));
        assert_eq!((p[3]["gridPos"]["x"].as_u64(), p[3]["gridPos"]["y"].as_u64()), (Some(0), Some(9)));
    }

    #[test]
    fn rendering_is_byte_stable_across_repeated_calls() {
        // The load-bearing invariant: no map may enter the marshal path,
        // so struct declaration order fully determines the bytes.
        let b = board();
        let first = serde_json::to_string(&b.render_grafana_json(&Theme::tundra()).unwrap()).unwrap();
        for _ in 0..16 {
            let again =
                serde_json::to_string(&b.render_grafana_json(&Theme::tundra()).unwrap()).unwrap();
            assert_eq!(again, first);
        }
    }

    #[test]
    fn a_panel_with_no_query_is_refused() {
        let b = Dashboard::new("x", "x")
            .datasource(ds())
            .row(Row::new("r").panel(Panel::new("empty", PanelKind::Stat, "Empty")));
        assert!(matches!(
            b.render_grafana_json(&Theme::tundra()),
            Err(DashboardError::PanelWithoutQuery { .. })
        ));
    }

    #[test]
    fn an_unregistered_datasource_is_refused() {
        let b = Dashboard::new("x", "x").row(
            Row::new("r").panel(
                Panel::new("p", PanelKind::Stat, "P").query(Query::new("A", "up", "nope")),
            ),
        );
        assert!(matches!(
            b.render_grafana_json(&Theme::tundra()),
            Err(DashboardError::UnknownDatasource { .. })
        ));
    }

    #[test]
    fn a_logs_query_against_a_metrics_datasource_is_refused() {
        // This is the incident the datasource registry exists to prevent:
        // a logs panel pointed at the Prometheus path renders as an error
        // in the browser, not as a build failure.
        let b = Dashboard::new("x", "x").datasource(ds()).row(
            Row::new("r").panel(
                Panel::new("p", PanelKind::Table, "Logs")
                    .query(Query::new("A", "{job=\"x\"} | json | line_format \"{{.msg}}\"", "mimir")),
            ),
        );
        assert!(matches!(
            b.render_grafana_json(&Theme::tundra()),
            Err(DashboardError::QueryLanguageMismatch { .. })
        ));
    }

    #[test]
    fn a_promql_regex_alternation_is_not_mistaken_for_a_logsql_pipe() {
        // `|` inside a quoted label matcher is PromQL regex alternation.
        // Classifying without blanking string literals first reads this as
        // LogsQL and rejects a perfectly ordinary Prometheus selector.
        let b = Dashboard::new("x", "x").datasource(ds()).row(
            Row::new("r").panel(
                Panel::new("p", PanelKind::Stat, "P")
                    .query(Query::new("A", "sum(rate(http_total{status=~\"500|503\"}[5m]))", "mimir")),
            ),
        );
        assert!(b.render_grafana_json(&Theme::tundra()).is_ok());
    }

    #[test]
    fn a_sql_datasource_emits_rawsql_not_expr() {
        // The ClickHouse plugin reads rawSql + queryType; an `expr` is
        // ignored and the panel renders empty.
        let b = Dashboard::new("ch", "ch")
            .datasource(Datasource::new("clickhouse", "grafana-clickhouse-datasource", QueryLang::Sql))
            .row(Row::new("r").panel(
                Panel::new("p", PanelKind::Table, "Rows").query(Query::new(
                    "A",
                    "SELECT count() FROM audit",
                    "clickhouse",
                )),
            ));
        let j = b.render_grafana_json(&Theme::tundra()).unwrap();
        let t = &j["panels"][1]["targets"][0];
        assert_eq!(t["rawSql"], "SELECT count() FROM audit");
        assert!(t["expr"].is_null());
        assert_eq!(t["queryType"], "table");
    }

    #[test]
    fn the_synthesizer_target_renders_the_same_document() {
        use crate::{GrafanaJson, Synthesizer};
        let b = board();
        let via_trait: serde_json::Value =
            Synthesizer::<GrafanaJson>::synthesize(&b).unwrap();
        assert_eq!(via_trait, b.render_grafana_json(&Theme::default()).unwrap());
    }
}
