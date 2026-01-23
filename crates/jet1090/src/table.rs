use chrono::prelude::*;
use ratatui::prelude::*;
use ratatui::widgets::*;
use regex::Regex;
use rs1090::data::patterns::aircraft_information;
use std::time::{SystemTime, UNIX_EPOCH};
use style::palette::tailwind;

use crate::snapshot::Snapshot;
use crate::{Jet1090, SortKey};

const INFO_TEXT: &str =
    "(Esc/Q) quit | (↑/K) up | (↓/J) down | (⤒/G) top | (/) search";

/**
 * Rendering of the table in interactive mode
 */
pub fn build_table(frame: &mut Frame, app: &mut Jet1090) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("SystemTime before unix epoch")
        .as_secs();

    // Update airport names
    app.receivers();

    let states = &app.state_vectors;

    // Get the interactive expire threshold (0 means no expiration)
    let interactive_expire = app.interactive_expire;

    // Filter items based on search query
    let search_query = app.search_query.to_lowercase().replace("-", "");
    let search_regex =
        Regex::new(&search_query).unwrap_or_else(|_| Regex::new("").unwrap());
    let filtered_states = states
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

    let rects = Layout::vertical([Constraint::Min(5), Constraint::Length(1)])
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
                .title_bottom(format!("jet1090 ({size} aircraft)",))
                .title_alignment(Alignment::Right)
                .title_style(Style::new().blue().bold())
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

    let area = rects[1];
    if app.is_search_mode {
        frame.render_widget(
            Paragraph::new(Line::from(format!(
                "Search (Esc to cancel, Enter to lock): {}",
                app.search_query
            )))
            .style(Style::new().fg(colors.row_fg).bg(colors.buffer_bg))
            .left_aligned(),
            area,
        );
    } else {
        frame.render_widget(
            Paragraph::new(Line::from(INFO_TEXT))
                .style(Style::new().fg(colors.row_fg).bg(colors.buffer_bg))
                .centered(),
            area,
        );
    }
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
