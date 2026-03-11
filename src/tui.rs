use crate::app::{
    format_compact, format_distance, format_relative, format_uptime, AggregatedDevice, App,
    SortColumn,
};
use crate::continuity::{ibeacon_uuid_name, ContinuityData};
use crate::service_uuids;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

fn rssi_color(rssi: i16) -> Color {
    if rssi >= -50 {
        Color::Green
    } else if rssi >= -70 {
        Color::Yellow
    } else {
        Color::Red
    }
}

fn header_cell(col: SortColumn, active: SortColumn, ascending: bool) -> Cell<'static> {
    let indicator = if col == active {
        if ascending {
            " ^"
        } else {
            " v"
        }
    } else {
        ""
    };
    let text = format!("{}{indicator}", col.label());
    let style = if col == active {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    };
    Cell::from(text).style(style)
}

pub fn draw(f: &mut Frame, app: &mut App) {
    if app.chat_mode {
        let chunks = Layout::vertical([
            Constraint::Length(1), // header
            Constraint::Min(5),   // chat messages
            Constraint::Length(1), // input
            Constraint::Length(1), // footer
        ])
        .split(f.area());

        draw_header(f, chunks[0], app);
        draw_chat_messages(f, chunks[1], app);
        draw_chat_input(f, chunks[2], app);
        draw_chat_footer(f, chunks[3]);
    } else {
        let area = f.area();
        let chunks = Layout::vertical([
            Constraint::Length(1), // header
            Constraint::Min(5),   // table
            Constraint::Length(1), // footer
        ])
        .split(area);

        draw_header(f, chunks[0], app);
        draw_table(f, chunks[1], app);
        draw_footer(f, chunks[2], app);

        // Overlay detail popup if active
        if app.detail_mode {
            draw_detail(f, area, app);
        }
    }
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let status = if app.scanning { "SCANNING" } else { "PAUSED" };
    let uptime = format_uptime(app.start_time);
    let spans = vec![
        Span::styled(
            " BLUEMON-TUI ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            format!(
                "Devices: {} ({} MACs)",
                app.fingerprint_groups.len(),
                app.devices.len()
            ),
            Style::default().fg(Color::White),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("Scan #{}", app.scan_count),
            Style::default().fg(Color::White),
        ),
        Span::raw(" | "),
        Span::styled(
            status,
            Style::default()
                .fg(if app.scanning {
                    Color::Green
                } else {
                    Color::Yellow
                })
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("Up: {uptime}"),
            Style::default().fg(Color::DarkGray),
        ),
    ];
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_table(f: &mut Frame, area: Rect, app: &mut App) {
    let sc = app.sort_column;
    let asc = app.sort_ascending;

    let header = Row::new(vec![
        header_cell(SortColumn::Distance, sc, asc),
        header_cell(SortColumn::Type, sc, asc),
        header_cell(SortColumn::Mac, sc, asc),
        header_cell(SortColumn::Vendor, sc, asc),
        header_cell(SortColumn::Name, sc, asc),
        Cell::from("Svcs").style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        header_cell(SortColumn::Rssi, sc, asc),
        header_cell(SortColumn::Sightings, sc, asc),
        Cell::from("FP").style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        header_cell(SortColumn::FirstSeen, sc, asc),
        header_cell(SortColumn::LastSeen, sc, asc),
        Cell::from("Activity 0h        23h").style(
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Note").style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ])
    .style(Style::default().bg(Color::DarkGray))
    .height(1);

    let rows: Vec<Row> = app
        .display_list
        .iter()
        .filter_map(|fp| app.aggregated.get(fp).map(|d| (fp, d)))
        .map(|(fp, d): (&String, &AggregatedDevice)| {
            let type_color = d.device_type.color();

            // MAC column: abbreviated to first 4 octets
            let mac_short = abbreviate_mac(&d.representative_mac);
            let mac_display = if d.mac_count > 1 {
                let suffix = format!(" (+{})", d.mac_count - 1);
                if d.is_randomized {
                    format!("*{mac_short}{suffix}")
                } else {
                    format!("{mac_short}{suffix}")
                }
            } else if d.is_randomized {
                format!("*{mac_short}")
            } else {
                mac_short
            };

            let svcs_display = service_uuids::resolve_compact(&d.service_uuids);

            let rssi_str = d.rssi.map(|r| r.to_string()).unwrap_or_default();
            let rssi_c = d.rssi.map(rssi_color).unwrap_or(Color::DarkGray);

            let dist_str = format_distance(d.rssi, d.tx_power, d.ibeacon_measured_power);
            let dist_c = d.rssi.map(rssi_color).unwrap_or(Color::DarkGray);

            let note_display = d.note.as_deref().unwrap_or("").to_string();

            // FP column: yellow if multiple MACs share this fingerprint
            let fp_color = if d.mac_count > 1 {
                Color::Yellow
            } else {
                Color::DarkGray
            };

            // Hourly activity sparkline
            let activity = app.hourly_cache.get(fp)
                .map(|counts| crate::app::format_hourly_sparkline(counts))
                .unwrap_or_default();

            Row::new(vec![
                Cell::from(dist_str).style(Style::default().fg(dist_c)),
                Cell::from(d.device_type.icon()).style(Style::default().fg(type_color)),
                Cell::from(mac_display),
                Cell::from(d.vendor.as_deref().unwrap_or("").to_string()),
                Cell::from(d.name.as_deref().unwrap_or("").to_string()),
                Cell::from(svcs_display).style(Style::default().fg(Color::Green)),
                Cell::from(rssi_str).style(Style::default().fg(rssi_c)),
                Cell::from(d.sightings.to_string()),
                Cell::from(d.fingerprint.clone()).style(Style::default().fg(fp_color)),
                Cell::from(format_compact(d.first_seen)),
                Cell::from(format_relative(d.last_seen)),
                Cell::from(activity).style(Style::default().fg(Color::Magenta)),
                Cell::from(note_display).style(Style::default().fg(Color::White)),
            ])
            .style(Style::default().fg(type_color))
        })
        .collect();

    let widths = [
        Constraint::Length(7),  // Dist
        Constraint::Length(5),  // Type
        Constraint::Length(16), // MAC
        Constraint::Min(10),   // Vendor
        Constraint::Min(8),    // Name
        Constraint::Min(10),   // Svcs
        Constraint::Length(5),  // RSSI
        Constraint::Length(4),  // Seen
        Constraint::Length(8),  // FP
        Constraint::Length(13), // First Seen
        Constraint::Length(8),  // Last Seen
        Constraint::Length(24), // Activity
        Constraint::Min(6),    // Note
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL))
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(table, area, &mut app.table_state);
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    if app.note_mode {
        let spans = vec![
            Span::styled(
                " Note ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(" {}: ", app.note_mac)),
            Span::styled(&app.note_input, Style::default().fg(Color::White)),
            Span::styled("_", Style::default().fg(Color::Yellow)),
        ];
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    } else if app.filter_mode {
        let text = format!("/{}", app.filter);
        f.render_widget(
            Paragraph::new(text).style(Style::default().fg(Color::Yellow)),
            area,
        );
    } else if let Some((msg, _)) = &app.probe_status {
        f.render_widget(
            Paragraph::new(msg.as_str())
                .style(Style::default().fg(Color::Cyan)),
            area,
        );
    } else {
        f.render_widget(
            Paragraph::new(
                "q:Quit  d:Detail  p:Probe  s/S:Sort  /:Filter  Enter:Note  e:CSV  E:JSON  j/k:Scroll",
            )
            .style(Style::default().fg(Color::DarkGray)),
            area,
        );
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let v = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(v[1])[1]
}

fn draw_detail(f: &mut Frame, area: Rect, app: &App) {
    let popup_area = centered_rect(70, 80, area);
    f.render_widget(Clear, popup_area);

    // Gather data for the selected device
    let Some(idx) = app.table_state.selected() else {
        return;
    };
    let Some(fp) = app.display_list.get(idx) else {
        return;
    };
    let Some(agg) = app.aggregated.get(fp) else {
        return;
    };
    let dev = app.devices.get(&agg.representative_mac);

    let label_style = Style::default().fg(Color::Cyan);
    let value_style = Style::default().fg(Color::White);
    let section_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Basic info
    let mac_display = if agg.mac_count > 1 {
        format!("{} (+{} MACs)", agg.representative_mac, agg.mac_count - 1)
    } else {
        agg.representative_mac.clone()
    };
    lines.push(Line::from(vec![
        Span::styled("MAC:         ", label_style),
        Span::styled(mac_display, value_style),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Name:        ", label_style),
        Span::styled(
            agg.name.as_deref().unwrap_or("(none)").to_string(),
            value_style,
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Vendor:      ", label_style),
        Span::styled(
            agg.vendor.as_deref().unwrap_or("(unknown)").to_string(),
            value_style,
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Type:        ", label_style),
        Span::styled(
            format!("{} {}", agg.device_type.icon(), agg.device_type.label()),
            Style::default().fg(agg.device_type.color()),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Fingerprint: ", label_style),
        Span::styled(agg.fingerprint.clone(), value_style),
    ]));
    let rssi_str = agg
        .rssi
        .map(|r| format!("{r} dBm"))
        .unwrap_or_else(|| "(none)".to_string());
    let dist_str = format_distance(agg.rssi, agg.tx_power, agg.ibeacon_measured_power);
    lines.push(Line::from(vec![
        Span::styled("RSSI:        ", label_style),
        Span::styled(format!("{rssi_str}  (Dist: {dist_str})"), value_style),
    ]));
    lines.push(Line::from(vec![
        Span::styled("First Seen:  ", label_style),
        Span::styled(format_compact(agg.first_seen), value_style),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Last Seen:   ", label_style),
        Span::styled(format_relative(agg.last_seen), value_style),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Sightings:   ", label_style),
        Span::styled(agg.sightings.to_string(), value_style),
    ]));
    if let Some(addr_type) = agg.addr_type {
        let (label, color) = match addr_type {
            crate::classifier::BleAddrType::Public => ("Public (OUI-assigned)", Color::Green),
            crate::classifier::BleAddrType::RandomStatic => ("Random Static", Color::Yellow),
            crate::classifier::BleAddrType::ResolvablePrivate => ("Resolvable Private (RPA)", Color::Yellow),
            crate::classifier::BleAddrType::NonResolvablePrivate => ("Non-Resolvable Private", Color::Red),
            crate::classifier::BleAddrType::Multicast => ("Multicast (anomalous)", Color::Red),
        };
        lines.push(Line::from(vec![
            Span::styled("Addr Type:   ", label_style),
            Span::styled(label, Style::default().fg(color)),
        ]));
    }
    if let Some(note) = &agg.note {
        lines.push(Line::from(vec![
            Span::styled("Note:        ", label_style),
            Span::styled(note.clone(), value_style),
        ]));
    }

    // Continuity / iBeacon section
    if let Some(ContinuityData::IBeacon { uuid, major, minor, measured_power }) = dev.and_then(|d| d.continuity.as_ref()) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("-- iBeacon --", section_style)));
        let uuid_label = if let Some(name) = ibeacon_uuid_name(uuid) {
            format!("{uuid} ({name})")
        } else {
            uuid.clone()
        };
        lines.push(Line::from(vec![
            Span::styled("  UUID:      ", label_style),
            Span::styled(uuid_label, value_style),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Major:     ", label_style),
            Span::styled(major.to_string(), value_style),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Minor:     ", label_style),
            Span::styled(minor.to_string(), value_style),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Measured:  ", label_style),
            Span::styled(format!("{measured_power} dBm"), value_style),
        ]));
    } else if let Some(summary) = &agg.continuity_summary {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "-- Continuity --",
            section_style,
        )));
        lines.push(Line::from(Span::styled(
            summary.clone(),
            Style::default().fg(Color::Green),
        )));
    }

    // GATT section
    if let Some(gatt) = &agg.gatt_info {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "-- GATT Device Info --",
            section_style,
        )));
        if let Some(v) = &gatt.manufacturer_name {
            lines.push(Line::from(vec![
                Span::styled("  Manufacturer: ", label_style),
                Span::styled(v.clone(), value_style),
            ]));
        }
        if let Some(v) = &gatt.model_number {
            lines.push(Line::from(vec![
                Span::styled("  Model:        ", label_style),
                Span::styled(v.clone(), value_style),
            ]));
        }
        if let Some(v) = &gatt.firmware_revision {
            lines.push(Line::from(vec![
                Span::styled("  Firmware:     ", label_style),
                Span::styled(v.clone(), value_style),
            ]));
        }
        if let Some(v) = &gatt.hardware_revision {
            lines.push(Line::from(vec![
                Span::styled("  Hardware:     ", label_style),
                Span::styled(v.clone(), value_style),
            ]));
        }
        if let Some(v) = &gatt.software_revision {
            lines.push(Line::from(vec![
                Span::styled("  Software:     ", label_style),
                Span::styled(v.clone(), value_style),
            ]));
        }
        if let Some(v) = &gatt.pnp_id {
            lines.push(Line::from(vec![
                Span::styled("  PnP ID:       ", label_style),
                Span::styled(v.clone(), value_style),
            ]));
        }
        if let Some(v) = &gatt.battery_level {
            lines.push(Line::from(vec![
                Span::styled("  Battery:      ", label_style),
                Span::styled(format!("{v}%"), value_style),
            ]));
        }
        lines.push(Line::from(vec![
            Span::styled("  Probed at:    ", label_style),
            Span::styled(gatt.probed_at.clone(), Style::default().fg(Color::DarkGray)),
        ]));
    }

    // Fast Pair section
    if let Some(model) = &agg.fast_pair_model {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "-- Fast Pair --",
            section_style,
        )));
        lines.push(Line::from(vec![
            Span::styled("  Model: ", label_style),
            Span::styled(model.clone(), value_style),
        ]));
    }

    // Manufacturer data hex dump
    if let Some(dev) = dev {
        if !dev.manufacturer_data.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "-- Manufacturer Data --",
                section_style,
            )));
            let mut ids: Vec<u16> = dev.manufacturer_data.keys().copied().collect();
            ids.sort();
            for id in ids {
                if let Some(data) = dev.manufacturer_data.get(&id) {
                    let hex: String = data
                        .iter()
                        .map(|b| format!("{:02X}", b))
                        .collect::<Vec<_>>()
                        .join(" ");
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {:04X}: ", id), label_style),
                        Span::styled(hex, Style::default().fg(Color::DarkGray)),
                    ]));
                }
            }
        }

        // Service UUIDs
        if !dev.service_uuids.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "-- Service UUIDs --",
                section_style,
            )));
            for uuid in &dev.service_uuids {
                if let Some(name) = crate::service_uuids::resolve(uuid) {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {uuid} "), Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("({name})"), Style::default().fg(Color::Green)),
                    ]));
                } else {
                    lines.push(Line::from(Span::styled(
                        format!("  {uuid}"),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
            }
        }
    }

    // Device class
    if let Some(dc) = agg.device_class {
        let major = (dc >> 8) & 0x1F;
        let minor = (dc >> 2) & 0x3F;
        let major_str = match major {
            1 => "Computer",
            2 => "Phone",
            3 => "LAN/Network",
            4 => "Audio/Video",
            5 => "Peripheral",
            6 => "Imaging",
            7 => "Wearable",
            8 => "Toy",
            9 => "Health",
            _ => "Other",
        };
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Device Class:", label_style),
            Span::styled(format!(" {major_str} ({major}/{minor}) [0x{dc:06X}]"), value_style),
        ]));
    }

    // RSSI sparkline
    if !app.detail_rssi_history.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "-- RSSI Trend (recent) --",
            section_style,
        )));
        let bars = [' ', '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}', '\u{2588}'];
        let min_rssi = app.detail_rssi_history.iter().copied().min().unwrap_or(-100) as f64;
        let max_rssi = app.detail_rssi_history.iter().copied().max().unwrap_or(-30) as f64;
        let range = max_rssi - min_rssi;
        let sparkline: String = app.detail_rssi_history.iter().map(|&r| {
            if range < 1.0 {
                // All values identical — show mid-level bar instead of invisible
                bars[4]
            } else {
                let norm = ((r as f64 - min_rssi) / range * 8.0) as usize;
                bars[norm.min(8)]
            }
        }).collect();
        lines.push(Line::from(vec![
            Span::styled("  ", label_style),
            Span::styled(sparkline, Style::default().fg(Color::Green)),
        ]));
        let last = app.detail_rssi_history.last().unwrap();
        lines.push(Line::from(vec![
            Span::styled(
                format!("  Range: {} to {} dBm  Latest: {} dBm  ({} samples)",
                    min_rssi as i16, max_rssi as i16, last, app.detail_rssi_history.len()),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    // Hourly activity heatmap
    if app.detail_hourly.iter().any(|&c| c > 0) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "-- Activity by Hour --",
            section_style,
        )));
        let max_count = *app.detail_hourly.iter().max().unwrap_or(&1) as f64;
        let bars = [' ', '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}', '\u{2588}'];
        let heatmap: String = app.detail_hourly.iter().map(|&c| {
            if c == 0 { '\u{2581}' }
            else {
                let norm = (c as f64 / max_count * 8.0) as usize;
                bars[norm.min(8)]
            }
        }).collect();
        lines.push(Line::from(vec![
            Span::styled("  ", label_style),
            Span::styled(heatmap, Style::default().fg(Color::Magenta)),
        ]));
        lines.push(Line::from(vec![
            Span::styled(
                format!("  0     6     12    18    23"),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    // MAC rotation stats
    if let Some(ref rot) = app.detail_rotation {
        if rot.total_macs > 1 {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "-- MAC Rotation --",
                section_style,
            )));
            lines.push(Line::from(vec![
                Span::styled("  Total MACs: ", label_style),
                Span::styled(rot.total_macs.to_string(), value_style),
            ]));
            if let Some(avg) = rot.avg_rotation_mins {
                let avg_str = if avg < 60.0 {
                    format!("{:.0} min", avg)
                } else {
                    format!("{:.1} hrs", avg / 60.0)
                };
                lines.push(Line::from(vec![
                    Span::styled("  Avg interval: ", label_style),
                    Span::styled(avg_str, value_style),
                ]));
            }
        }
    }

    // Apply scrolling
    let inner_height = popup_area.height.saturating_sub(2) as usize;
    let total = lines.len();
    let max_scroll = total.saturating_sub(inner_height);
    let scroll = app.detail_scroll.min(max_scroll);
    let visible: Vec<Line<'static>> = lines.into_iter().skip(scroll).take(inner_height).collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Device Detail ")
        .title_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(Color::Cyan));

    f.render_widget(
        Paragraph::new(visible).block(block).wrap(Wrap { trim: false }),
        popup_area,
    );
}

fn draw_chat_messages(f: &mut Frame, area: Rect, app: &App) {
    let inner_height = area.height.saturating_sub(2) as usize; // account for block borders

    // Collect all rendered lines
    let mut all_lines: Vec<Line<'static>> = Vec::new();
    for (i, msg) in app.chat.messages.iter().enumerate() {
        if i > 0 {
            all_lines.push(Line::from(""));
        }
        all_lines.extend(msg.rendered.clone());
    }

    if app.chat.waiting {
        if !all_lines.is_empty() {
            all_lines.push(Line::from(""));
        }
        all_lines.push(Line::from(Span::styled(
            "Thinking...",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
    }

    // Scroll: offset from bottom
    let total = all_lines.len();
    let skip = if total > inner_height {
        let max_scroll = total - inner_height;
        let scroll = app.chat.scroll_offset.min(max_scroll);
        total - inner_height - scroll
    } else {
        0
    };

    let visible: Vec<Line<'static>> = all_lines
        .into_iter()
        .skip(skip)
        .take(inner_height)
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Chat ")
        .title_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );

    f.render_widget(Paragraph::new(visible).block(block), area);
}

fn draw_chat_input(f: &mut Frame, area: Rect, app: &App) {
    let (prompt, input_style) = if app.chat.waiting {
        ("... ", Style::default().fg(Color::DarkGray))
    } else {
        ("> ", Style::default().fg(Color::White))
    };

    let spans = vec![
        Span::styled(
            prompt,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(app.chat.input.as_str().to_string(), input_style),
        Span::styled("_", Style::default().fg(Color::Cyan)),
    ];

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_chat_footer(f: &mut Frame, area: Rect) {
    f.render_widget(
        Paragraph::new("Esc:Back  Enter:Send  Up/Down:Scroll")
            .style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

/// Abbreviate a MAC address to its first 4 octets: `AA:BB:CC:DD..`
fn abbreviate_mac(mac: &str) -> String {
    let parts: Vec<&str> = mac.split(':').collect();
    if parts.len() > 4 {
        format!("{}:{}:{}:{}..", parts[0], parts[1], parts[2], parts[3])
    } else {
        mac.to_string()
    }
}
