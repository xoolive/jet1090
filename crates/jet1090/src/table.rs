use chrono::prelude::*;
use ratatui::prelude::*;
use ratatui::widgets::*;
use regex::Regex;
use rs1090::data::patterns::aircraft_information;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};
use style::palette::tailwind;
use tokio::sync::RwLockReadGuard;

use crate::health::{HealthState, Severity, SourceStatus};
use crate::snapshot::{Snapshot, StateVectors};
use crate::{Jet1090, SharedState, SortKey};

const INFO_TEXT: &str =
    "(Esc/Q) quit | (↑/K) up | (↓/J) down | (⤒/G) top | (/) search | (S) sources | (E) errors";

/**
 * Rendering of the table in interactive mode
 */
pub fn build_table(
    frame: &mut Frame,
    app: &mut Jet1090,
    shared: &SharedState,
    state_vectors: &RwLockReadGuard<'_, BTreeMap<String, StateVectors>>,
) {
    // Keep the summary, sources and errors consistent without holding the
    // health mutex while rendering widgets.
    let health = shared
        .health
        .lock()
        .expect("health state lock poisoned")
        .clone();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("SystemTime before unix epoch")
        .as_secs();

    // Get the interactive expire threshold (0 means no expiration)
    let interactive_expire = app.interactive_expire;

    // Filter items based on search query
    let search_query = app.search_query.to_lowercase().replace("-", "");
    let search_regex =
        Regex::new(&search_query).unwrap_or_else(|_| Regex::new("").unwrap());
    let filtered_states = state_vectors
        .values()
        .filter(|sv| {
            (sv.cur.count > 1)
                    // Only show aircraft seen within the expiration threshold (or always if 0)
                    && (interactive_expire == 0 || (now as i64 - sv.cur.lastseen as i64) < interactive_expire as i64)
                    && (sv.cur.callsign.as_ref().is_some_and(|s| {
                        search_regex.is_match(&s.to_lowercase())
                    }) || search_regex
                        .is_match(&sv.cur.icao24.to_lowercase())
                        || sv.cur.typecode.as_ref().is_some_and(|s| {
                            search_regex.is_match(&s.to_lowercase())
                        })
                        || sv.cur.registration.as_ref().is_some_and(|s| {
                            search_regex
                                .is_match(&s.replace("-", "").to_lowercase())
                        })
                        || sv.cur.metadata.iter().any(|m| {
                            m.name.as_ref().is_some_and(|n| {
                                search_regex.is_match(&n.to_lowercase())
                            })
                        }))
        })
        .collect::<Vec<&StateVectors>>();

    app.items = filtered_states
        .iter()
        .map(|sv| sv.cur.icao24.to_string())
        .collect();

    app.scroll_state = app.scroll_state.content_length(app.items.len());

    let rects = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(source_panel_height(
            frame.area().height,
            app,
            &health,
        )),
        Constraint::Length(1),
    ])
    .split(frame.area());
    let colors = TableColors::new(&tailwind::CYAN);

    use crate::snapshot::StateVectors;
    let mut sorted_elts = filtered_states;

    let sort_by = match &app.sort_key {
        SortKey::ALTITUDE => |a: &&StateVectors, b: &&StateVectors| {
            a.cur.altitude.cmp(&b.cur.altitude)
        },
        SortKey::CALLSIGN => |a: &&StateVectors, b: &&StateVectors| {
            a.cur.callsign.cmp(&b.cur.callsign)
        },
        SortKey::VRATE => |a: &&StateVectors, b: &&StateVectors| {
            a.cur.vertical_rate.cmp(&b.cur.vertical_rate)
        },
        SortKey::COUNT => {
            |a: &&StateVectors, b: &&StateVectors| a.cur.count.cmp(&b.cur.count)
        }
        SortKey::FIRST => |a: &&StateVectors, b: &&StateVectors| {
            a.cur.firstseen.cmp(&b.cur.firstseen)
        },
        SortKey::LAST => |a: &&StateVectors, b: &&StateVectors| {
            a.cur.lastseen.cmp(&b.cur.lastseen)
        },
    };

    sorted_elts.sort_by(sort_by);
    if !&app.sort_asc {
        sorted_elts.reverse();
    }
    let columns = {
        use ColumnRender::*;
        match app.width {
            w if w <= 70 => {
                vec![
                    ICAO24,
                    CALLSIGN,
                    LATITUDE,
                    LONGITUDE,
                    ALTITUDE,
                    GROUNDSPEED,
                    TRACK,
                ]
            }
            w if w <= 80 => {
                vec![
                    ICAO24,
                    CALLSIGN,
                    LATITUDE,
                    LONGITUDE,
                    ALTITUDE,
                    GROUNDSPEED,
                    TRACK,
                    LAST,
                ]
            }
            w if w <= 100 => {
                vec![
                    ICAO24,
                    CALLSIGN,
                    SQUAWK,
                    LATITUDE,
                    LONGITUDE,
                    ALTITUDE,
                    GROUNDSPEED,
                    VRATE,
                    TRACK,
                    LAST,
                    FIRST,
                ]
            }
            w if w <= 120 => {
                vec![
                    ICAO24,
                    CALLSIGN,
                    SQUAWK,
                    LATITUDE,
                    LONGITUDE,
                    ALTITUDE,
                    GROUNDSPEED,
                    VRATE,
                    TRACK,
                    NACP,
                    LAST,
                    FIRST,
                ]
            }
            w if w <= 130 => {
                vec![
                    ICAO24,
                    CALLSIGN,
                    TYPECODE,
                    SQUAWK,
                    LATITUDE,
                    LONGITUDE,
                    ALTITUDE,
                    SELALT,
                    GROUNDSPEED,
                    TAS,
                    IAS,
                    MACH,
                    VRATE,
                    TRACK,
                    HEADING,
                    ROLL,
                    NACP,
                    LAST,
                    FIRST,
                ]
            }
            _ => {
                let mut cols = vec![ICAO24];
                // Insert FLAG before TAIL only when flags are enabled
                if app.flags {
                    cols.push(FLAG);
                }
                cols.extend([
                    TAIL,
                    CALLSIGN,
                    TYPECODE,
                    SQUAWK,
                    LATITUDE,
                    LONGITUDE,
                    ALTITUDE,
                    SELALT,
                    GROUNDSPEED,
                    TAS,
                    IAS,
                    MACH,
                    VRATE,
                    TRACK,
                    HEADING,
                    ROLL,
                    NACP,
                    COUNT,
                    REFERENCE,
                    LAST,
                    FIRST,
                ]);
                cols
            }
        }
    };
    let rows = sorted_elts
        .iter()
        // Only show aircraft seen within the expiration threshold (or always if 0)
        .filter(|sv| {
            interactive_expire == 0
                || (now as i64 - sv.cur.lastseen as i64)
                    < interactive_expire as i64
        })
        .enumerate()
        .map(|(i, sv)| {
            let color = match i % 2 {
                0 => colors.normal_row_color,
                _ => colors.alt_row_color,
            };
            columns
                .iter()
                .map(|c| c.cell(&sv.cur, now))
                .collect::<Row<'_>>()
                .style(Style::new().fg(colors.row_fg).bg(color))
        })
        .collect::<Vec<Row<'_>>>();

    let size = &rows.len();
    let bar = "█";

    let header = columns
        .iter()
        .map(|c| c.header(&app.sort_key))
        .collect::<Vec<Cell<'_>>>();

    let constraints = columns
        .iter()
        .map(|c| c.constraint())
        .collect::<Vec<Constraint>>();

    let health_title = source_health_title(&health);
    let table = Table::new(rows, constraints)
        .column_spacing(2)
        .header(
            Row::new(header)
                //.bottom_margin(1)
                .style(
                    Style::default()
                        .fg(colors.header_fg)
                        .bg(colors.header_bg)
                        .bold(),
                ),
        )
        .block(
            Block::default()
                .title_bottom(health_title)
                .title_bottom(
                    Line::from(Span::styled(
                        format!("jet1090 ({size} aircraft) "),
                        Style::new().blue().bold(),
                    ))
                    .alignment(Alignment::Right),
                )
                .padding(Padding::symmetric(1, 0))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        )
        .bg(colors.buffer_bg)
        .row_highlight_style(
            Style::default()
                .add_modifier(Modifier::REVERSED)
                .fg(colors.selected_style_fg),
        )
        .highlight_symbol(bar)
        .highlight_spacing(HighlightSpacing::Always);

    let area = rects[0];
    frame.render_stateful_widget(table, area, &mut app.state);

    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None),
        area.inner(Margin {
            vertical: 1,
            horizontal: 0,
        }),
        &mut app.scroll_state,
    );

    if app.show_sources {
        render_sources(frame, rects[1], &health);
    }

    let controls_area = rects[2];
    let controls = if app.is_search_mode {
        Line::from(format!(
            "Search (Esc to cancel, Enter to lock): {}",
            app.search_query
        ))
    } else {
        Line::from(INFO_TEXT)
    };
    frame.render_widget(
        Paragraph::new(controls)
            .style(Style::new().fg(colors.row_fg).bg(colors.buffer_bg))
            .centered(),
        controls_area,
    );

    if app.show_errors {
        render_error_overlay(frame, app, &health);
    }
}

fn source_panel_height(
    height: u16,
    app: &Jet1090,
    health: &HealthState,
) -> u16 {
    if !app.show_sources {
        return 0;
    }
    let rows = health
        .sources
        .len()
        .max(1)
        .saturating_mul(2)
        .saturating_add(2);
    rows.min(usize::from(height / 2)) as u16
}

fn render_sources(frame: &mut Frame, area: Rect, health: &HealthState) {
    let mut lines = Vec::new();
    for (source, status) in &health.sources {
        let (symbol, label, color) = match status {
            SourceStatus::Healthy => ("●", "connected", tailwind::GREEN.c400),
            SourceStatus::Connecting => {
                ("○", "connecting", tailwind::CYAN.c400)
            }
            SourceStatus::Reconnecting => {
                ("↻", "reconnecting", tailwind::YELLOW.c400)
            }
            SourceStatus::Failed => ("✕", "failed", tailwind::RED.c400),
        };
        let mut text = format!("{symbol} {source}: {label}");
        if matches!(status, SourceStatus::Reconnecting | SourceStatus::Failed) {
            if let Some(event) = health
                .events
                .iter()
                .rev()
                .find(|event| event.source == *source)
            {
                text.push_str(&format!(" — {}", event.message));
            }
        }
        lines.push(Line::styled(text, Style::new().fg(color)));
    }
    if lines.is_empty() {
        lines.push(Line::raw("No sources configured."));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .style(
                Style::new()
                    .fg(tailwind::SLATE.c200)
                    .bg(tailwind::SLATE.c950),
            )
            .wrap(Wrap { trim: false })
            .block(
                Block::bordered()
                    .title(" Sources (S) hide ")
                    .border_type(BorderType::Rounded),
            ),
        area,
    );
}

fn source_health_title(health: &HealthState) -> Line<'static> {
    let mut healthy = 0;
    let mut connecting = 0;
    let mut reconnecting = 0;
    let mut failed = 0;
    for status in health.sources.values() {
        match status {
            SourceStatus::Healthy => healthy += 1,
            SourceStatus::Connecting => connecting += 1,
            SourceStatus::Reconnecting => reconnecting += 1,
            SourceStatus::Failed => failed += 1,
        }
    }

    let mut spans = vec![
        Span::raw(" "),
        Span::styled(
            format!("● {healthy}"),
            Style::new().fg(tailwind::GREEN.c400),
        ),
        Span::raw(" "),
        Span::styled(
            format!("○ {connecting}"),
            Style::new().fg(tailwind::CYAN.c400),
        ),
        Span::raw(" "),
        Span::styled(
            format!("↻ {reconnecting}"),
            Style::new().fg(tailwind::YELLOW.c400),
        ),
        Span::raw(" "),
        Span::styled(
            format!("✕ {failed}"),
            Style::new().fg(tailwind::RED.c400),
        ),
    ];
    if health.unread_errors > 0 {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!("⚠ {}", health.unread_errors),
            Style::new().fg(tailwind::RED.c400).bold(),
        ));
    }
    Line::from(spans)
}

fn render_error_overlay(
    frame: &mut Frame,
    app: &mut Jet1090,
    health: &HealthState,
) {
    let area = centered_rect(85, 75, frame.area());
    let height = area.height.saturating_sub(2) as usize;
    let visible_events = health
        .events
        .iter()
        .rev()
        .skip(app.error_scroll)
        .take(height)
        .collect::<Vec<_>>();
    let event_count = health.events.len();
    let events = visible_events
        .iter()
        .map(|event| {
            let color = match event.severity {
                Severity::Warning => tailwind::YELLOW.c400,
                Severity::Error => tailwind::RED.c400,
            };
            let repeats = if event.occurrences > 1 {
                format!(" (×{})", event.occurrences)
            } else {
                String::new()
            };
            Line::from(vec![
                Span::styled(
                    event.timestamp.format("%H:%M:%S").to_string(),
                    Style::new().fg(tailwind::SLATE.c400),
                ),
                Span::raw("  "),
                Span::styled(
                    event.severity.label(),
                    Style::new().fg(color).bold(),
                ),
                Span::raw("  "),
                Span::styled(
                    event.source.clone(),
                    Style::new().fg(tailwind::CYAN.c300),
                ),
                Span::raw("  "),
                Span::raw(format!("{}{}", event.message, repeats)),
            ])
        })
        .collect::<Vec<_>>();

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(events)
            .block(
                Block::default()
                    .title(format!(" Recent errors ({event_count}) "))
                    .title_style(Style::new().fg(tailwind::RED.c400).bold())
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(tailwind::RED.c400)),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}

/**
 * Style-sheet of the table displayed in interactive mode
 */
struct TableColors {
    buffer_bg: Color,
    header_bg: Color,
    header_fg: Color,
    row_fg: Color,
    selected_style_fg: Color,
    normal_row_color: Color,
    alt_row_color: Color,
    //footer_border_color: Color,
}

impl TableColors {
    const fn new(color: &tailwind::Palette) -> Self {
        Self {
            buffer_bg: tailwind::SLATE.c950,
            header_bg: color.c900,
            header_fg: tailwind::SLATE.c200,
            row_fg: tailwind::SLATE.c200,
            selected_style_fg: color.c400,
            normal_row_color: tailwind::SLATE.c950,
            alt_row_color: tailwind::SLATE.c900,
            //footer_border_color: color.c400,
        }
    }
}

trait Render {
    fn cell(&self, snapshot: &Snapshot, now: u64) -> String;
    fn header(&self, sort_key: &SortKey) -> Cell<'_>;
    fn constraint(&self) -> Constraint;
}

/**
 * Describes how to render information in a given column
 */
#[allow(clippy::upper_case_acronyms)]
enum ColumnRender {
    ICAO24,
    FLAG,
    TAIL,
    CALLSIGN,
    TYPECODE,
    SQUAWK,
    LATITUDE,
    LONGITUDE,
    ALTITUDE,
    SELALT,
    GROUNDSPEED,
    TAS,
    IAS,
    MACH,
    VRATE,
    TRACK,
    HEADING,
    ROLL,
    NACP,
    COUNT,
    REFERENCE,
    LAST,
    FIRST,
}

impl Render for ColumnRender {
    fn cell(&self, s: &Snapshot, now: u64) -> String {
        match self {
            Self::ICAO24 => s.icao24.to_string(),
            Self::FLAG => {
                // Get flag emoji from ICAO24 address
                if let Ok(info) = aircraft_information(&s.icao24, None) {
                    info.flag.unwrap_or_default()
                } else {
                    String::new()
                }
            }
            Self::TAIL => s.registration.to_owned().unwrap_or("".to_string()),
            Self::CALLSIGN => s.callsign.to_owned().unwrap_or("".to_string()),
            Self::TYPECODE => s.typecode.to_owned().unwrap_or("".to_string()),
            Self::SQUAWK => {
                s.squawk.map(|s| s.to_string()).unwrap_or("".to_string())
            }
            Self::LATITUDE => {
                s.latitude.map(|v| format!("{v}")).unwrap_or("".to_string())
            }
            Self::LONGITUDE => s
                .longitude
                .map(|v| format!("{v}"))
                .unwrap_or("".to_string()),
            Self::ALTITUDE => match (s.altitude, &s.airport) {
                (Some(alt), _) => format!("{alt}"),
                (None, Some(airport)) => airport.clone(),
                (None, None) => "".to_string(),
            },
            Self::SELALT => match (s.selected_altitude, s.altitude) {
                (Some(sel), Some(alt))
                    if i32::abs_diff(i32::from(sel), alt) <= 50 =>
                {
                    "=".to_string()
                }
                (Some(sel), _) => {
                    format!("{}", sel / 100)
                }
                _ => "".to_string(),
            },
            Self::GROUNDSPEED => s
                .groundspeed
                .map(|v| format!("{v}"))
                .unwrap_or("".to_string()),
            Self::TAS => {
                s.tas.map(|v| format!("{v}")).unwrap_or("".to_string())
            }
            Self::IAS => {
                s.ias.map(|v| format!("{v}")).unwrap_or("".to_string())
            }
            Self::MACH => {
                s.mach.map(|v| format!("{v}")).unwrap_or("".to_string())
            }
            Self::VRATE => s
                .vertical_rate
                .map(|v| format!("{v}"))
                .unwrap_or("".to_string()),
            Self::TRACK => {
                s.track.map(|v| format!("{v}")).unwrap_or("".to_string())
            }
            Self::HEADING => {
                s.heading.map(|v| format!("{v}")).unwrap_or("".to_string())
            }
            Self::ROLL => {
                s.roll.map(|v| format!("{v}")).unwrap_or("".to_string())
            }
            Self::NACP => {
                s.nacp.map(|v| format!("{v}")).unwrap_or("".to_string())
            }
            Self::COUNT => s.count.to_string(),
            Self::REFERENCE => s
                .metadata
                .first()
                .unwrap()
                .name
                .clone()
                .unwrap_or("".to_string()),
            Self::LAST => {
                if now > s.lastseen + 5 {
                    format!("{}s ago", now - s.lastseen)
                } else {
                    "".to_string()
                }
            }
            Self::FIRST => {
                let dt: DateTime<Utc> =
                    DateTime::from_timestamp(s.firstseen as i64, 0).unwrap();
                format!("{}", dt.format("%H:%M"))
            }
        }
    }

    fn header(&self, sort_key: &SortKey) -> Cell<'_> {
        match self {
            ColumnRender::ICAO24 => Cell::from("icao24".to_string()),
            ColumnRender::FLAG => Cell::from("".to_string()), // No header for flag column
            ColumnRender::TAIL => Cell::from("tail".to_string()),
            ColumnRender::CALLSIGN => {
                let mut c = Cell::from("callsign".to_string());
                if *sort_key == SortKey::CALLSIGN {
                    c = c.fg(tailwind::AMBER.c400);
                }
                c
            }
            ColumnRender::TYPECODE => Cell::from("type".to_string()),
            ColumnRender::SQUAWK => Cell::from("sqwk".to_string()),
            ColumnRender::LATITUDE => Cell::from("lat".to_string()),
            ColumnRender::LONGITUDE => Cell::from("lon".to_string()),
            ColumnRender::ALTITUDE => {
                let mut c = Cell::from("alt".to_string());
                if *sort_key == SortKey::ALTITUDE {
                    c = c.fg(tailwind::AMBER.c400);
                }
                c
            }
            ColumnRender::SELALT => Cell::from("sel".to_string()),
            ColumnRender::GROUNDSPEED => Cell::from("gs".to_string()),
            ColumnRender::TAS => Cell::from("tas".to_string()),
            ColumnRender::IAS => Cell::from("ias".to_string()),
            ColumnRender::MACH => Cell::from("mach".to_string()),
            ColumnRender::VRATE => {
                let mut c = Cell::from("vrate".to_string());
                if *sort_key == SortKey::VRATE {
                    c = c.fg(tailwind::AMBER.c400);
                }
                c
            }
            ColumnRender::TRACK => Cell::from("trk".to_string()),
            ColumnRender::HEADING => Cell::from("hdg".to_string()),
            ColumnRender::ROLL => Cell::from("roll".to_string()),
            ColumnRender::NACP => Cell::from("nac".to_string()),
            ColumnRender::COUNT => Cell::from("count".to_string()),
            ColumnRender::REFERENCE => Cell::from("ref".to_string()),
            ColumnRender::LAST => {
                let mut c = Cell::from("last".to_string());
                if *sort_key == SortKey::LAST {
                    c = c.fg(tailwind::AMBER.c400);
                }
                c
            }
            ColumnRender::FIRST => {
                let mut c = Cell::from("first".to_string());
                if *sort_key == SortKey::FIRST {
                    c = c.fg(tailwind::AMBER.c400);
                }
                c
            }
        }
    }
    fn constraint(&self) -> Constraint {
        match self {
            ColumnRender::ICAO24 => Constraint::Length(6),
            ColumnRender::FLAG => Constraint::Length(2),
            ColumnRender::TAIL => Constraint::Length(8),
            ColumnRender::CALLSIGN => Constraint::Length(8),
            ColumnRender::TYPECODE => Constraint::Length(4),
            ColumnRender::SQUAWK => Constraint::Length(4),
            ColumnRender::LATITUDE => Constraint::Length(6),
            ColumnRender::LONGITUDE => Constraint::Length(6),
            ColumnRender::ALTITUDE => Constraint::Length(5),
            ColumnRender::SELALT => Constraint::Length(3),
            ColumnRender::GROUNDSPEED => Constraint::Length(3),
            ColumnRender::TAS => Constraint::Length(3),
            ColumnRender::IAS => Constraint::Length(3),
            ColumnRender::MACH => Constraint::Length(4),
            ColumnRender::VRATE => Constraint::Length(5),
            ColumnRender::TRACK => Constraint::Length(5),
            ColumnRender::HEADING => Constraint::Length(5),
            ColumnRender::ROLL => Constraint::Length(5),
            ColumnRender::NACP => Constraint::Length(3),
            ColumnRender::COUNT => Constraint::Length(8),
            ColumnRender::REFERENCE => Constraint::Length(8),
            ColumnRender::LAST => Constraint::Length(7),
            ColumnRender::FIRST => Constraint::Length(5),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::{self, HealthEvent};
    use ratatui::backend::TestBackend;

    fn shared_with_sources() -> SharedState {
        let health = health::new_health();
        for (name, status) in [
            ("Airspy", SourceStatus::Healthy),
            ("Local", SourceStatus::Connecting),
            ("Remote", SourceStatus::Reconnecting),
            ("Missing", SourceStatus::Failed),
        ] {
            health::set_source_status(&health, name, status);
        }
        for (source, message) in [
            ("Airspy", "stale error after recovery"),
            ("Remote", "older failure"),
            ("Remote", "connection refused"),
            ("Missing", "device unavailable"),
            ("other module", "unrelated error"),
        ] {
            health.lock().unwrap().events.push_back(HealthEvent {
                timestamp: Local::now(),
                severity: Severity::Error,
                source: source.to_string(),
                message: message.to_string(),
                occurrences: 1,
            });
        }
        SharedState::new(BTreeMap::new(), health)
    }

    fn draw(
        width: u16,
        height: u16,
        app: &mut Jet1090,
        shared: &SharedState,
    ) -> ratatui::buffer::Buffer {
        app.width = width;
        let mut terminal =
            Terminal::new(TestBackend::new(width, height)).unwrap();
        let states = shared.state_vectors.blocking_read();
        terminal
            .draw(|frame| build_table(frame, app, shared, &states))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn text(buffer: &ratatui::buffer::Buffer) -> String {
        buffer.content.iter().map(|cell| cell.symbol()).collect()
    }

    #[test]
    fn sources_panel_is_hidden_by_default() {
        let buffer =
            draw(120, 30, &mut Jet1090::default(), &shared_with_sources());
        let output = text(&buffer);
        assert!(output.contains("(S) sources"));
        assert!(!output.contains("Sources (S) hide"));
        assert!(!output.contains("device unavailable"));
    }

    #[test]
    fn sources_panel_shows_statuses_and_current_reasons() {
        let mut app = Jet1090 {
            show_sources: true,
            ..Jet1090::default()
        };
        let buffer = draw(120, 30, &mut app, &shared_with_sources());
        let output = text(&buffer);
        for expected in [
            "Sources (S) hide",
            "Airspy",
            "Local",
            "Remote",
            "Missing",
            "connected",
            "connecting",
            "reconnecting",
            "failed",
            "connection refused",
            "device unavailable",
            "jet1090 (0 aircraft)",
        ] {
            assert!(output.contains(expected), "missing {expected}: {output}");
        }
        for obsolete in [
            "older failure",
            "stale error after recovery",
            "unrelated error",
        ] {
            assert!(!output.contains(obsolete));
        }
        for (symbol, color) in [
            ("●", tailwind::GREEN.c400),
            ("○", tailwind::CYAN.c400),
            ("↻", tailwind::YELLOW.c400),
            ("✕", tailwind::RED.c400),
        ] {
            // Check the sources panel, not the existing summary in the
            // aircraft table's bottom border (row 18).
            assert!(buffer
                .content
                .iter()
                .skip(120 * 19)
                .take(120 * 10)
                .any(|cell| cell.symbol() == symbol && cell.fg == color));
        }
    }

    #[test]
    fn recovered_sources_stop_showing_old_failure_reasons() {
        let shared = shared_with_sources();
        let mut app = Jet1090 {
            show_sources: true,
            ..Jet1090::default()
        };
        assert!(text(&draw(120, 30, &mut app, &shared))
            .contains("connection refused"));
        health::set_source_status(
            &shared.health,
            "Remote",
            SourceStatus::Healthy,
        );
        assert!(!text(&draw(120, 30, &mut app, &shared))
            .contains("connection refused"));
        assert!(shared
            .health
            .lock()
            .unwrap()
            .events
            .iter()
            .any(|event| event.message == "connection refused"));
    }

    #[test]
    fn sources_panel_preserves_aircraft_table() {
        let shared = shared_with_sources();
        let cur = Snapshot {
            icao24: "abc123".to_string(),
            firstseen: 0,
            lastseen: 0,
            callsign: None,
            registration: None,
            typecode: None,
            squawk: None,
            latitude: None,
            longitude: None,
            altitude: None,
            selected_altitude: None,
            groundspeed: None,
            vertical_rate: None,
            track: None,
            ias: None,
            tas: None,
            mach: None,
            roll: None,
            heading: None,
            nacp: None,
            count: 2,
            metadata: Vec::new(),
            airport: None,
        };
        shared.state_vectors.blocking_write().insert(
            cur.icao24.clone(),
            StateVectors {
                cur,
                hist: Vec::new(),
            },
        );
        let mut app = Jet1090 {
            show_sources: true,
            ..Jet1090::default()
        };
        let output = text(&draw(120, 30, &mut app, &shared));
        assert!(output.contains("abc123"));
        assert!(output.contains("jet1090 (1 aircraft)"));
        assert!(output.contains("Sources (S) hide"));
    }

    #[test]
    fn sources_panel_handles_empty_source_registry() {
        let shared = SharedState::new(BTreeMap::new(), health::new_health());
        let mut app = Jet1090 {
            show_sources: true,
            ..Jet1090::default()
        };
        assert!(text(&draw(120, 24, &mut app, &shared))
            .contains("No sources configured"));
    }

    #[test]
    fn tiny_terminals_render_with_sources_and_errors() {
        let shared = shared_with_sources();
        for width in [0, 1, 2, 10, 40] {
            for height in [0, 1, 2, 3, 5, 8] {
                for show_sources in [false, true] {
                    for show_errors in [false, true] {
                        let mut app = Jet1090 {
                            show_sources,
                            show_errors,
                            ..Jet1090::default()
                        };
                        draw(width, height, &mut app, &shared);
                    }
                }
            }
        }
    }
}
