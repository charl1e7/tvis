//! UI-thread side: owns the Slint models, applies [`ViewState`]s and handles
//! every callback coming from the views.
//!
//! Models are created once and updated row by row, so Slint only re-renders
//! the elements whose values actually changed.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel, Weak};
use tvis_core::alert::{AlertRule, Comparison};
use tvis_core::engine::Command;
use tvis_core::export::ExportFormat;
use tvis_core::metric::{MetricRegistry, Unit};
use tvis_core::units::{ByteBase, format_value};
use tvis_core::{Target, TargetId};

use super::view::*;
use crate::settings::{
    INTERVALS_MS, RETENTIONS_SECS, Settings, SettingsStore, ThemeMode, WINDOWS_SECS,
};
use crate::{
    AlertLogRow, AlertRuleRow, AppWindow, Backend, ChartData, ChartLine, CoreBar, HoverItem,
    InsightRow, MetricCard, MonitorHeader, ProcRow, StatItem, SystemView, Theme, TopRow, TreeRow,
    WatchRow,
};

thread_local! {
    static UI: RefCell<Option<Rc<Ui>>> = const { RefCell::new(None) };
}

/// Runs `f` with the UI state; only valid on the UI thread after [`Ui::install`].
pub fn with_ui(f: impl FnOnce(&Rc<Ui>)) {
    UI.with(|slot| {
        if let Some(ui) = slot.borrow().as_ref() {
            f(ui);
        }
    });
}

fn rgb(v: u32) -> slint::Color {
    slint::Color::from_rgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Replaces the contents of `model` with `new`, touching only rows that differ.
fn sync<T: Clone + PartialEq + 'static>(model: &VecModel<T>, new: Vec<T>) {
    let old = model.row_count();
    for (i, row) in new.iter().enumerate() {
        if i < old {
            if model.row_data(i).as_ref() != Some(row) {
                model.set_row_data(i, row.clone());
            }
        } else {
            model.push(row.clone());
        }
    }
    while model.row_count() > new.len() {
        model.remove(model.row_count() - 1);
    }
}

struct ChartModels {
    lines: Rc<VecModel<ChartLine>>,
    y: Rc<VecModel<crate::Tick>>,
    x: Rc<VecModel<crate::Tick>>,
}

impl ChartModels {
    fn new() -> Self {
        Self {
            lines: Rc::new(VecModel::default()),
            y: Rc::new(VecModel::default()),
            x: Rc::new(VecModel::default()),
        }
    }

    fn data(&self, empty: &str) -> ChartData {
        ChartData {
            lines: ModelRc::from(self.lines.clone()),
            y_ticks: ModelRc::from(self.y.clone()),
            x_ticks: ModelRc::from(self.x.clone()),
            empty_text: empty.into(),
        }
    }

    fn apply(&self, chart: Option<&ChartV>) {
        let Some(c) = chart else {
            sync(&self.lines, vec![]);
            sync(&self.y, vec![]);
            sync(&self.x, vec![]);
            return;
        };
        let ticks = |t: &[Tick]| {
            t.iter()
                .map(|t| crate::Tick {
                    frac: t.frac,
                    label: t.label.as_str().into(),
                })
                .collect::<Vec<_>>()
        };
        sync(
            &self.lines,
            c.lines
                .iter()
                .map(|l| ChartLine {
                    name: l.name.as_str().into(),
                    color: rgb(l.color),
                    stroke: l.stroke.as_str().into(),
                    fill: l.fill.as_str().into(),
                    last: l.last.as_str().into(),
                })
                .collect(),
        );
        sync(&self.y, ticks(&c.y_ticks));
        sync(&self.x, ticks(&c.x_ticks));
    }
}

pub struct Ui {
    window: Weak<AppWindow>,
    request: SharedRequest,
    engine: crossbeam_channel::Sender<Command>,
    store: SettingsStore,
    settings: RefCell<Settings>,
    registry: MetricRegistry,

    // models
    watchlist: Rc<VecModel<WatchRow>>,
    cards: Rc<VecModel<MetricCard>>,
    stats: Rc<VecModel<StatItem>>,
    insights: Rc<VecModel<InsightRow>>,
    tree: Rc<VecModel<TreeRow>>,
    core_bars: Rc<VecModel<CoreBar>>,
    top_cpu: Rc<VecModel<TopRow>>,
    top_mem: Rc<VecModel<TopRow>>,
    procs: Rc<VecModel<ProcRow>>,
    rules: Rc<VecModel<AlertRuleRow>>,
    alert_log: Rc<VecModel<AlertLogRow>>,
    hover_items: Rc<VecModel<HoverItem>>,
    target_labels: Rc<VecModel<SharedString>>,
    metric_labels: Rc<VecModel<SharedString>>,
    metric_choices: Rc<VecModel<crate::Choice>>,
    main_chart: ChartModels,
    cpu_chart: ChartModels,
    mem_chart: ChartModels,

    // state mirrored from the last update
    chart_cache: RefCell<Option<ChartV>>,
    targets: RefCell<Vec<(String, String)>>,
    firing: RefCell<Vec<u64>>,
    save_timer: Timer,
    toast_timer: Timer,
    toast_seq: Cell<u64>,
}

impl Ui {
    #[allow(clippy::too_many_arguments)]
    pub fn install(
        window: &AppWindow,
        request: SharedRequest,
        engine: crossbeam_channel::Sender<Command>,
        store: SettingsStore,
        settings: Settings,
        registry: MetricRegistry,
        engine_info: String,
    ) -> Rc<Self> {
        let ui = Rc::new(Self {
            window: window.as_weak(),
            request,
            engine,
            store,
            settings: RefCell::new(settings),
            registry,
            watchlist: Rc::default(),
            cards: Rc::default(),
            stats: Rc::default(),
            insights: Rc::default(),
            tree: Rc::default(),
            core_bars: Rc::default(),
            top_cpu: Rc::default(),
            top_mem: Rc::default(),
            procs: Rc::default(),
            rules: Rc::default(),
            alert_log: Rc::default(),
            hover_items: Rc::default(),
            target_labels: Rc::default(),
            metric_labels: Rc::default(),
            metric_choices: Rc::default(),
            main_chart: ChartModels::new(),
            cpu_chart: ChartModels::new(),
            mem_chart: ChartModels::new(),
            chart_cache: RefCell::default(),
            targets: RefCell::default(),
            firing: RefCell::default(),
            save_timer: Timer::default(),
            toast_timer: Timer::default(),
            toast_seq: Cell::new(0),
        });
        ui.bind_models(window, &engine_info);
        ui.push_settings_to_ui(window);
        ui.rebuild_rules();
        ui.sync_request();
        ui.wire(window);
        UI.with(|slot| *slot.borrow_mut() = Some(ui.clone()));
        ui
    }

    fn bind_models(&self, w: &AppWindow, engine_info: &str) {
        let b = w.global::<Backend>();
        b.set_watchlist(ModelRc::from(self.watchlist.clone()));
        b.set_cards(ModelRc::from(self.cards.clone()));
        b.set_stats(ModelRc::from(self.stats.clone()));
        b.set_insights(ModelRc::from(self.insights.clone()));
        b.set_tree(ModelRc::from(self.tree.clone()));
        b.set_cores(ModelRc::from(self.core_bars.clone()));
        b.set_top_cpu(ModelRc::from(self.top_cpu.clone()));
        b.set_top_mem(ModelRc::from(self.top_mem.clone()));
        b.set_procs(ModelRc::from(self.procs.clone()));
        b.set_rules(ModelRc::from(self.rules.clone()));
        b.set_alert_log(ModelRc::from(self.alert_log.clone()));
        b.set_hover_items(ModelRc::from(self.hover_items.clone()));
        b.set_alert_target_labels(ModelRc::from(self.target_labels.clone()));
        b.set_alert_metric_labels(ModelRc::from(self.metric_labels.clone()));
        b.set_alert_metrics(ModelRc::from(self.metric_choices.clone()));
        b.set_chart(self.main_chart.data("Waiting for samples…"));
        b.set_cpu_chart(self.cpu_chart.data("Waiting for samples…"));
        b.set_mem_chart(self.mem_chart.data("Waiting for samples…"));
        b.set_version(env!("CARGO_PKG_VERSION").into());
        b.set_data_dir(self.store.dir().display().to_string().into());
        b.set_engine_info(engine_info.into());
        sync(&self.target_labels, vec!["All watched targets".into()]);
        let metrics: Vec<_> = self.registry.iter().collect();
        sync(
            &self.metric_labels,
            metrics.iter().map(|d| d.name.into()).collect(),
        );
        sync(
            &self.metric_choices,
            metrics
                .iter()
                .map(|d| crate::Choice {
                    id: d.id.as_str().into(),
                    label: d.name.into(),
                    hint: threshold_unit(d.unit).0.into(),
                })
                .collect(),
        );
    }

    // ── settings ↔ UI ────────────────────────────────────────────────────

    fn push_settings_to_ui(&self, w: &AppWindow) {
        let s = self.settings.borrow();
        let b = w.global::<Backend>();
        b.set_theme_mode(s.theme.index());
        b.set_accent_index(s.accent as i32);
        b.set_interval_index(s.interval_index() as i32);
        b.set_retention_index(s.retention_index() as i32);
        b.set_smoothing(i32::from(s.smoothing));
        b.set_chart_fill(s.chart_fill);
        b.set_chart_grid(s.chart_grid);
        b.set_line_width(i32::from(s.line_width));
        b.set_byte_base(i32::from(s.byte_base == ByteBase::Decimal));
        b.set_cpu_normalized(s.cpu_normalized);
        b.set_window_index(s.window_index() as i32);
        let t = w.global::<Theme>();
        t.set_mode(s.theme.index());
        t.set_accent(rgb(ACCENTS[s.accent.min(ACCENTS.len() - 1)]));
    }

    fn settings_changed(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let b = w.global::<Backend>();
        let (interval_changed, retention_changed);
        {
            let mut s = self.settings.borrow_mut();
            let (old_i, old_r) = (s.interval_ms, s.retention_secs);
            s.theme = ThemeMode::from_index(b.get_theme_mode());
            s.accent = b.get_accent_index().clamp(0, 5) as usize;
            s.interval_ms = INTERVALS_MS[b.get_interval_index().clamp(0, 5) as usize];
            s.retention_secs = RETENTIONS_SECS[b.get_retention_index().clamp(0, 4) as usize];
            s.smoothing = b.get_smoothing().clamp(0, 5) as u8;
            s.chart_fill = b.get_chart_fill();
            s.chart_grid = b.get_chart_grid();
            s.line_width = b.get_line_width().clamp(0, 2) as u8;
            s.byte_base = if b.get_byte_base() == 1 {
                ByteBase::Decimal
            } else {
                ByteBase::Binary
            };
            s.cpu_normalized = b.get_cpu_normalized();
            s.window_secs = WINDOWS_SECS[b.get_window_index().clamp(0, 4) as usize];
            interval_changed = s.interval_ms != old_i;
            retention_changed = s.retention_secs != old_r;
            let t = w.global::<Theme>();
            t.set_mode(s.theme.index());
            t.set_accent(rgb(ACCENTS[s.accent]));
        }
        let s = self.settings.borrow().clone();
        if interval_changed {
            let _ = self
                .engine
                .send(Command::SetInterval(Duration::from_millis(s.interval_ms)));
        }
        if retention_changed {
            let _ = self
                .engine
                .send(Command::SetRetention(Duration::from_secs(s.retention_secs)));
        }
        self.sync_request();
        self.schedule_save();
    }

    fn schedule_save(&self) {
        let store = self.store.clone();
        let settings = self.settings.clone();
        // Debounce: slider drags would otherwise write the file on every step.
        self.save_timer.start(
            TimerMode::SingleShot,
            Duration::from_millis(400),
            move || {
                if let Err(e) = store.save(&settings.borrow()) {
                    tracing::warn!("could not save settings: {e}");
                }
            },
        );
    }

    /// Writes pending changes immediately (used on exit).
    pub fn flush(&self) {
        self.save_timer.stop();
        if let Err(e) = self.store.save(&self.settings.borrow()) {
            tracing::warn!("could not save settings: {e}");
        }
    }

    // ── request ──────────────────────────────────────────────────────────

    /// Reads every view-related property and tells the engine to re-publish.
    pub fn sync_request(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let b = w.global::<Backend>();
        let s = self.settings.borrow();
        {
            let mut r = self.request.lock().unwrap();
            r.page = Page::from_index(b.get_page());
            r.window_secs = WINDOWS_SECS[b.get_window_index().clamp(0, 4) as usize];
            r.chart_view = ChartView::from_index(b.get_chart_view());
            r.tree_flat = b.get_tree_mode() == 1;
            r.show_threads = b.get_show_threads();
            r.proc_filter = b.get_proc_filter().to_string();
            r.proc_group = b.get_proc_group();
            r.smoothing = f64::from(s.smoothing) * 0.15;
            r.byte_base = s.byte_base;
            r.cpu_normalized = s.cpu_normalized;
            let px = (w.window().size().width as f64 - 420.0).max(400.0);
            r.chart_px = px as usize;
        }
        let _ = self.engine.send(Command::Refresh);
    }

    fn update_request(&self, f: impl FnOnce(&mut ViewRequest)) {
        f(&mut self.request.lock().unwrap());
        let _ = self.engine.send(Command::Refresh);
    }

    // ── applying engine output ───────────────────────────────────────────

    pub fn apply(&self, s: ViewState) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let b = w.global::<Backend>();
        b.set_sampling_label(s.sampling_label.as_str().into());
        b.set_host(s.host.as_str().into());
        b.set_live(s.live);
        b.set_paused(s.paused);
        b.set_alerts_firing(s.firing_rules.len() as i32);

        sync(
            &self.watchlist,
            s.watchlist
                .iter()
                .map(|r| WatchRow {
                    id: r.id.as_str().into(),
                    title: r.title.as_str().into(),
                    subtitle: r.subtitle.as_str().into(),
                    running: r.running,
                    cpu: r.cpu.as_str().into(),
                    mem: r.mem.as_str().into(),
                    spark: r.spark.as_str().into(),
                    selected: r.selected,
                    alerting: r.alerting,
                })
                .collect(),
        );
        let mut labels: Vec<SharedString> = vec!["All watched targets".into()];
        labels.extend(s.targets.iter().map(|(_, t)| t.as_str().into()));
        sync(&self.target_labels, labels);
        *self.targets.borrow_mut() = s.targets;
        if *self.firing.borrow() != s.firing_rules {
            *self.firing.borrow_mut() = s.firing_rules;
            self.rebuild_rules();
        }

        if let Some(d) = s.dashboard {
            self.apply_dashboard(&b, d);
        }
        if let Some(p) = s.processes {
            b.set_procs_summary(p.summary.as_str().into());
            sync(
                &self.procs,
                p.rows
                    .iter()
                    .map(|r| ProcRow {
                        key: r.key.as_str().into(),
                        name: r.name.as_str().into(),
                        pid: r.pid.as_str().into(),
                        cpu: r.cpu.as_str().into(),
                        cpu_frac: r.cpu_frac,
                        mem: r.mem.as_str().into(),
                        mem_frac: r.mem_frac,
                        threads: r.threads.as_str().into(),
                        watched: r.watched,
                        is_pid: r.is_pid,
                    })
                    .collect(),
            );
        }
        let on_monitor = self.request.lock().unwrap().page == Page::Monitor;
        match s.monitor {
            Some(m) => self.apply_monitor(&b, m),
            None if on_monitor => {
                b.set_has_selection(false);
                self.clear_monitor();
            }
            None => {}
        }
    }

    fn clear_monitor(&self) {
        sync(&self.cards, vec![]);
        sync(&self.stats, vec![]);
        sync(&self.insights, vec![]);
        sync(&self.tree, vec![]);
        self.main_chart.apply(None);
        *self.chart_cache.borrow_mut() = None;
    }

    fn apply_dashboard(&self, b: &Backend<'_>, d: DashboardV) {
        let s = &d.system;
        b.set_system(SystemView {
            cpu: s.cpu.as_str().into(),
            cpu_frac: s.cpu_frac,
            cpu_spark: s.cpu_spark.as_str().into(),
            mem: s.mem.as_str().into(),
            mem_detail: s.mem_detail.as_str().into(),
            mem_frac: s.mem_frac,
            mem_spark: s.mem_spark.as_str().into(),
            swap: s.swap.as_str().into(),
            swap_detail: s.swap_detail.as_str().into(),
            swap_frac: s.swap_frac,
            load: s.load.as_str().into(),
            load_detail: s.load_detail.as_str().into(),
            uptime: s.uptime.as_str().into(),
            processes: s.processes.as_str().into(),
            cpu_model: s.cpu_model.as_str().into(),
        });
        sync(
            &self.core_bars,
            d.cores
                .iter()
                .map(|(l, f, v)| CoreBar {
                    label: l.as_str().into(),
                    frac: *f,
                    value: v.as_str().into(),
                })
                .collect(),
        );
        self.cpu_chart.apply(d.cpu_chart.as_ref());
        self.mem_chart.apply(d.mem_chart.as_ref());
        let top = |rows: &[TopRowV]| {
            rows.iter()
                .map(|r| TopRow {
                    name: r.name.as_str().into(),
                    value: r.value.as_str().into(),
                    frac: r.frac,
                    count: r.count.as_str().into(),
                    key: r.key.as_str().into(),
                    watched: r.watched,
                })
                .collect::<Vec<_>>()
        };
        sync(&self.top_cpu, top(&d.top_cpu));
        sync(&self.top_mem, top(&d.top_mem));
    }

    fn apply_monitor(&self, b: &Backend<'_>, m: MonitorV) {
        b.set_has_selection(true);
        b.set_monitor(MonitorHeader {
            title: m.title.as_str().into(),
            subtitle: m.subtitle.as_str().into(),
            running: m.running,
            id: m.id.as_str().into(),
            metric_name: SharedString::new(),
            metric_unit_hint: SharedString::new(),
        });
        b.set_chart_title(m.chart_title.as_str().into());
        sync(
            &self.cards,
            m.cards
                .iter()
                .map(|c| MetricCard {
                    id: c.id.as_str().into(),
                    name: c.name.as_str().into(),
                    value: c.value.as_str().into(),
                    detail: c.detail.as_str().into(),
                    trend: c.trend.as_str().into(),
                    trend_dir: c.trend_dir,
                    spark: c.spark.as_str().into(),
                    color: rgb(c.color),
                    selected: c.selected,
                })
                .collect(),
        );
        sync(
            &self.stats,
            m.stats
                .iter()
                .map(|(l, v)| StatItem {
                    label: l.as_str().into(),
                    value: v.as_str().into(),
                })
                .collect(),
        );
        sync(
            &self.insights,
            m.insights
                .iter()
                .map(|i| InsightRow {
                    severity: i.severity,
                    title: i.title.as_str().into(),
                    detail: i.detail.as_str().into(),
                })
                .collect(),
        );
        sync(
            &self.tree,
            m.tree
                .iter()
                .map(|r| TreeRow {
                    pid: r.pid.as_str().into(),
                    name: r.name.as_str().into(),
                    detail: r.detail.as_str().into(),
                    depth: r.depth,
                    is_thread: r.is_thread,
                    cpu: r.cpu.as_str().into(),
                    cpu_frac: r.cpu_frac,
                    mem: r.mem.as_str().into(),
                    mem_frac: r.mem_frac,
                    threads: r.threads.as_str().into(),
                    io: r.io.as_str().into(),
                    spark: r.spark.as_str().into(),
                })
                .collect(),
        );
        self.main_chart.apply(m.chart.as_ref());
        *self.chart_cache.borrow_mut() = m.chart;
    }

    // ── alerts ───────────────────────────────────────────────────────────

    fn rebuild_rules(&self) {
        let firing = self.firing.borrow();
        let targets = self.targets.borrow();
        let rows = self
            .settings
            .borrow()
            .alerts
            .iter()
            .map(|r| {
                let unit = self
                    .registry
                    .find(&r.metric)
                    .map_or(Unit::Count, |d| d.unit);
                let name = self
                    .registry
                    .find(&r.metric)
                    .map_or(r.metric.as_str(), |d| d.name);
                let base = self.settings.borrow().byte_base;
                AlertRuleRow {
                    id: r.id as i32,
                    name: r.name.as_str().into(),
                    summary: format!(
                        "{name} {} {} for {} s",
                        if r.comparison == Comparison::Above {
                            "above"
                        } else {
                            "below"
                        },
                        format_value(unit, r.threshold, base),
                        r.for_secs
                    )
                    .into(),
                    scope: match &r.target {
                        None => "all targets".into(),
                        Some(id) => targets
                            .iter()
                            .find(|(i, _)| i == id)
                            .map_or(id.clone(), |(_, t)| t.clone()),
                    }
                    .into(),
                    enabled: r.enabled,
                    firing: firing.contains(&r.id),
                }
            })
            .collect();
        sync(&self.rules, rows);
    }

    fn push_rules_to_engine(&self) {
        let rules = self.settings.borrow().alerts.clone();
        let _ = self.engine.send(Command::SetAlertRules(rules));
        self.rebuild_rules();
        self.schedule_save();
    }

    pub fn alert_event(&self, fired: bool, text: String) {
        let time = chrono::Local::now().format("%H:%M:%S").to_string();
        self.alert_log.insert(
            0,
            AlertLogRow {
                time: time.into(),
                text: text.as_str().into(),
                kind: i32::from(fired),
            },
        );
        while self.alert_log.row_count() > 200 {
            self.alert_log.remove(self.alert_log.row_count() - 1);
        }
        if fired {
            self.toast(2, &text);
        }
    }

    pub fn toast(&self, kind: i32, text: &str) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let b = w.global::<Backend>();
        b.set_toast_kind(kind);
        b.set_toast_text(text.into());
        let seq = self.toast_seq.get() + 1;
        self.toast_seq.set(seq);
        let weak = self.window.clone();
        self.toast_timer
            .start(TimerMode::SingleShot, Duration::from_secs(6), move || {
                if let Some(w) = weak.upgrade() {
                    w.global::<Backend>().set_toast_text(SharedString::new());
                }
            });
    }

    // ── callbacks ────────────────────────────────────────────────────────

    fn wire(self: &Rc<Self>, w: &AppWindow) {
        let b = w.global::<Backend>();
        macro_rules! on {
            ($method:ident, |$ui:ident $(, $arg:ident)*| $body:block) => {{
                let ui = self.clone();
                b.$method(move |$($arg),*| {
                    let $ui = &ui;
                    $body
                });
            }};
        }

        on!(on_page_changed, |ui, _p| {
            ui.sync_request();
        });
        on!(on_view_changed, |ui| {
            ui.settings_changed();
        });
        on!(on_settings_changed, |ui| {
            ui.settings_changed();
        });
        on!(on_toggle_pause, |ui| {
            if let Some(w) = ui.window.upgrade() {
                let paused = !w.global::<Backend>().get_paused();
                w.global::<Backend>().set_paused(paused);
                let _ = ui.engine.send(Command::SetPaused(paused));
            }
        });
        on!(on_dismiss_toast, |ui| {
            if let Some(w) = ui.window.upgrade() {
                w.global::<Backend>().set_toast_text(SharedString::new());
            }
        });

        // monitor
        on!(on_select_target, |ui, id| {
            ui.update_request(|r| r.selected = Some(TargetId::from(id.as_str())));
        });
        on!(on_select_metric, |ui, id| {
            ui.update_request(|r| r.metric = id.to_string());
        });
        on!(on_unwatch, |ui, id| {
            ui.unwatch(id.as_str());
        });
        on!(on_clear_history, |ui, id| {
            let _ = ui
                .engine
                .send(Command::ClearHistory(Some(TargetId::from(id.as_str()))));
        });
        on!(on_export_target, |ui, id, fmt| {
            ui.export(
                id.as_str(),
                if fmt == 1 {
                    ExportFormat::Json
                } else {
                    ExportFormat::Csv
                },
            );
        });
        on!(on_chart_hover, |ui, f| {
            ui.hover(f);
        });
        on!(on_chart_leave, |ui| {
            ui.leave();
        });
        on!(on_open_data_dir, |ui| {
            if let Err(e) = opener::open(ui.store.dir()) {
                ui.toast(2, &format!("Cannot open folder: {e}"));
            }
        });

        // processes
        on!(on_proc_filter_changed, |ui, _t| {
            ui.sync_request();
        });
        on!(on_sort_procs, |ui, col| {
            let sort = ProcSort::from_index(col);
            let (sort_now, asc_now) = {
                let r = ui.request.lock().unwrap();
                (r.proc_sort, r.proc_ascending)
            };
            let ascending = if sort == sort_now {
                !asc_now
            } else {
                sort == ProcSort::Name
            };
            ui.update_request(|r| {
                r.proc_sort = sort;
                r.proc_ascending = ascending;
            });
            if let Some(w) = ui.window.upgrade() {
                let b = w.global::<Backend>();
                b.set_proc_sort_col(sort.index());
                b.set_proc_sort_asc(ascending);
            }
        });
        on!(on_watch_process, |ui, key, is_pid| {
            let target = if is_pid {
                key.parse().map(Target::Pid).ok()
            } else {
                Some(Target::Name(key.to_string()))
            };
            if let Some(t) = target {
                ui.watch(t);
            }
        });
        on!(on_watch_text, |ui, text| {
            if let Some(t) = Target::parse(text.as_str()) {
                ui.watch(t)
            }
        });

        // alerts
        on!(on_add_alert, |ui,
                           name,
                           target_idx,
                           metric_idx,
                           comparison,
                           threshold,
                           secs| {
            ui.add_alert(
                name.as_str(),
                target_idx,
                metric_idx,
                comparison,
                threshold,
                secs,
            );
        });
        on!(on_remove_alert, |ui, id| {
            ui.settings
                .borrow_mut()
                .alerts
                .retain(|r| r.id != id as u64);
            ui.push_rules_to_engine();
        });
        on!(on_toggle_alert, |ui, id, enabled| {
            if let Some(r) = ui
                .settings
                .borrow_mut()
                .alerts
                .iter_mut()
                .find(|r| r.id == id as u64)
            {
                r.enabled = enabled;
            }
            ui.push_rules_to_engine();
        });
        on!(on_clear_alert_log, |ui| {
            sync(&ui.alert_log, vec![]);
        });
        on!(on_reset_settings, |ui| {
            let watchlist = ui.settings.borrow().watchlist.clone();
            *ui.settings.borrow_mut() = Settings {
                watchlist,
                ..Settings::default()
            };
            ui.settings.borrow_mut().alerts.clear();
            if let Some(w) = ui.window.upgrade() {
                ui.push_settings_to_ui(&w);
            }
            let _ = ui
                .engine
                .send(Command::SetInterval(Duration::from_millis(1000)));
            let _ = ui
                .engine
                .send(Command::SetRetention(Duration::from_secs(3600)));
            ui.push_rules_to_engine();
            ui.sync_request();
            ui.toast(1, "Settings restored to defaults");
        });
    }

    fn watch(&self, target: Target) {
        let id = target.id();
        {
            let mut s = self.settings.borrow_mut();
            if matches!(target, Target::Name(_)) && !s.watchlist.iter().any(|t| t.id() == id) {
                s.watchlist.push(target.clone());
            }
        }
        let label = target.to_string();
        let _ = self.engine.send(Command::Watch(target));
        self.update_request(|r| r.selected = Some(id));
        self.schedule_save();
        self.toast(1, &format!("Watching {label}"));
    }

    fn unwatch(&self, id: &str) {
        self.settings
            .borrow_mut()
            .watchlist
            .retain(|t| t.id().as_str() != id);
        let _ = self.engine.send(Command::Unwatch(TargetId::from(id)));
        self.update_request(|r| {
            if r.selected.as_ref().is_some_and(|s| s.as_str() == id) {
                r.selected = None;
            }
        });
        self.schedule_save();
    }

    fn add_alert(
        &self,
        name: &str,
        target_idx: i32,
        metric_idx: i32,
        comparison: i32,
        threshold: f32,
        secs: f32,
    ) {
        let Some(desc) = self.registry.iter().nth(metric_idx.max(0) as usize) else {
            return;
        };
        if !threshold.is_finite() || threshold < 0.0 {
            self.toast(2, "Enter a threshold first");
            return;
        }
        let (_, multiplier) = threshold_unit(desc.unit);
        let target = (target_idx > 0)
            .then(|| {
                self.targets
                    .borrow()
                    .get(target_idx as usize - 1)
                    .map(|(id, _)| id.clone())
            })
            .flatten();
        let mut s = self.settings.borrow_mut();
        let id = s.alerts.iter().map(|r| r.id).max().unwrap_or(0) + 1;
        let cmp = if comparison == 1 {
            Comparison::Below
        } else {
            Comparison::Above
        };
        let for_secs = f64::from(secs.max(0.0));
        let threshold = f64::from(threshold) * multiplier;
        s.alerts.push(AlertRule {
            id,
            name: if name.trim().is_empty() {
                format!(
                    "{} {}",
                    desc.name,
                    if cmp == Comparison::Above {
                        "high"
                    } else {
                        "low"
                    }
                )
            } else {
                name.trim().to_owned()
            },
            target,
            metric: desc.id.as_str().to_owned(),
            comparison: cmp,
            threshold,
            for_secs,
            enabled: true,
        });
        drop(s);
        self.push_rules_to_engine();
        self.toast(1, "Alert rule added");
    }

    fn export(&self, id: &str, format: ExportFormat) {
        let dir = export_dir();
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let name: String = id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let path = dir.join(format!("tvis-{name}-{stamp}.{}", format.extension()));
        let _ = self.engine.send(Command::Export {
            target: TargetId::from(id),
            format,
            path,
        });
    }

    // ── hover ────────────────────────────────────────────────────────────

    fn hover(&self, frac: f32) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let b = w.global::<Backend>();
        let cache = self.chart_cache.borrow();
        let Some(chart) = cache.as_ref().filter(|c| !c.lines.is_empty()) else {
            return;
        };

        let nearest = |pts: &[(f32, f64)]| -> Option<(f32, f64)> {
            let i = pts.partition_point(|p| p.0 < frac);
            let cands = [i.checked_sub(1), Some(i)]
                .into_iter()
                .flatten()
                .filter_map(|j| pts.get(j));
            cands
                .min_by(|a, c| (a.0 - frac).abs().total_cmp(&(c.0 - frac).abs()))
                .copied()
        };
        let mut items = Vec::new();
        let mut x = None;
        for line in &chart.lines {
            if let Some((px, v)) = nearest(&line.points) {
                x.get_or_insert(px);
                items.push(HoverItem {
                    name: line.name.as_str().into(),
                    color: rgb(line.color),
                    value: format_value(chart.unit, v, chart.base).into(),
                });
            }
        }
        let Some(x) = x else { return };
        let ago = f64::from(1.0 - x) * chart.window_secs;
        b.set_hover_time(
            if ago < 0.5 {
                "now".to_owned()
            } else {
                format!("−{}", tvis_core::units::format_duration(ago))
            }
            .into(),
        );
        b.set_hover_frac(x);
        sync(&self.hover_items, items);
    }

    fn leave(&self) {
        if let Some(w) = self.window.upgrade() {
            w.global::<Backend>().set_hover_frac(-1.0);
        }
    }

    pub fn select_target(&self, id: TargetId) {
        self.update_request(|r| r.selected = Some(id));
    }

    pub fn select_page(&self, page: i32) {
        if let Some(w) = self.window.upgrade() {
            w.global::<Backend>().set_page(page);
        }
        self.sync_request();
    }
}

/// `(label, multiplier to base unit)` for thresholds typed into the alert form.
fn threshold_unit(unit: Unit) -> (&'static str, f64) {
    match unit {
        Unit::Percent => ("% of a core", 1.0),
        Unit::Bytes => ("MiB", 1024.0 * 1024.0),
        Unit::BytesPerSec => ("MiB/s", 1024.0 * 1024.0),
        Unit::Count => ("", 1.0),
    }
}

fn export_dir() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|d| d.document_dir().map(|p| p.to_path_buf()))
        .unwrap_or_else(std::env::temp_dir)
        .join("TVIS")
}
