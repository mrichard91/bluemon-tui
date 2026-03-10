use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

// ── Data types ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
    System,
}

#[derive(Clone)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub raw_text: String,
    pub rendered: Vec<Line<'static>>,
}

pub enum ChatEvent {
    AssistantMessage {
        text: String,
        response_id: Option<String>,
    },
    Error(String),
}

// ── Chat state ──────────────────────────────────────────────────────────

pub struct ChatState {
    pub messages: Vec<ChatMessage>,
    pub input: String,
    pub scroll_offset: usize,
    pub waiting: bool,
    tx: mpsc::UnboundedSender<ChatEvent>,
    rx: mpsc::UnboundedReceiver<ChatEvent>,
    previous_response_id: Option<String>,
    api_key: Option<String>,
    db_path: String,
}

impl ChatState {
    pub fn new(db_path: String) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let api_key = std::env::var("OPENAI_API_KEY").ok().filter(|k| !k.is_empty());
        Self {
            messages: Vec::new(),
            input: String::new(),
            scroll_offset: 0,
            waiting: false,
            tx,
            rx,
            previous_response_id: None,
            api_key,
            db_path,
        }
    }

    pub fn send_message(&mut self) {
        if self.waiting || self.input.trim().is_empty() {
            return;
        }

        let api_key = match &self.api_key {
            Some(k) => k.clone(),
            None => {
                self.messages.push(ChatMessage {
                    role: ChatRole::System,
                    raw_text: "Set OPENAI_API_KEY environment variable to use chat.".into(),
                    rendered: render_system_message(
                        "Set OPENAI_API_KEY environment variable to use chat.",
                    ),
                });
                return;
            }
        };

        let text = std::mem::take(&mut self.input);
        self.messages.push(ChatMessage {
            role: ChatRole::User,
            raw_text: text.clone(),
            rendered: render_user_message(&text),
        });

        self.waiting = true;
        self.scroll_offset = 0;

        let tx = self.tx.clone();
        let db_path = self.db_path.clone();
        let prev_id = self.previous_response_id.clone();

        tokio::spawn(async move {
            let event = run_chat_turn(&api_key, &db_path, &text, prev_id).await;
            let _ = tx.send(event);
        });
    }

    pub fn drain_events(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                ChatEvent::AssistantMessage { text, response_id } => {
                    self.previous_response_id = response_id;
                    self.messages.push(ChatMessage {
                        role: ChatRole::Assistant,
                        raw_text: text.clone(),
                        rendered: render_assistant_message(&text),
                    });
                    self.waiting = false;
                    self.scroll_offset = 0;
                }
                ChatEvent::Error(err) => {
                    self.messages.push(ChatMessage {
                        role: ChatRole::System,
                        raw_text: err.clone(),
                        rendered: render_system_message(&err),
                    });
                    self.waiting = false;
                }
            }
        }
    }

    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(3);
    }

    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(3);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    /// Total rendered lines across all messages (including spacing).
    pub fn total_lines(&self) -> usize {
        let mut n = 0;
        for msg in &self.messages {
            n += msg.rendered.len() + 1; // +1 for blank line between messages
        }
        if self.waiting {
            n += 1; // "Thinking..." line
        }
        n
    }
}

// ── OpenAI Responses API types ──────────────────────────────────────────

#[derive(Serialize)]
struct ApiRequest {
    model: &'static str,
    instructions: String,
    input: serde_json::Value,
    tools: Vec<ToolDef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_response_id: Option<String>,
}

#[derive(Serialize, Clone)]
struct ToolDef {
    r#type: &'static str,
    name: &'static str,
    description: &'static str,
    parameters: serde_json::Value,
}

#[derive(Deserialize)]
struct ApiResponse {
    id: Option<String>,
    #[serde(default)]
    output: Vec<OutputItem>,
    #[serde(default)]
    error: Option<ApiError>,
}

#[derive(Deserialize)]
struct ApiError {
    message: Option<String>,
}

#[derive(Deserialize)]
struct OutputItem {
    r#type: Option<String>,
    // For message items
    content: Option<Vec<ContentPart>>,
    // For function_call items
    call_id: Option<String>,
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct ContentPart {
    r#type: Option<String>,
    text: Option<String>,
}

// ── Async API call ──────────────────────────────────────────────────────

fn build_system_prompt() -> String {
    let now = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z");
    format!(
        r#"You are a Bluetooth scan data analyst. The current local time is {now}.

## Database Schema

CREATE TABLE devices (
    mac TEXT PRIMARY KEY,
    name TEXT,
    vendor TEXT,
    device_type TEXT NOT NULL DEFAULT 'unknown',
    is_randomized INTEGER NOT NULL DEFAULT 0,
    first_seen TEXT NOT NULL,       -- RFC3339 local datetime e.g. "2025-06-15T14:30:00+01:00"
    last_seen TEXT NOT NULL,        -- RFC3339 local datetime
    note TEXT DEFAULT '',
    service_uuids TEXT DEFAULT '',  -- Comma-separated UUID list
    sightings INTEGER NOT NULL DEFAULT 0,
    tx_power INTEGER,
    fingerprint TEXT DEFAULT '',    -- 4-char hex, groups randomized MACs belonging to the same physical device
    continuity_json TEXT DEFAULT '', -- JSON: Apple Continuity parsed data (type, battery levels, etc.)
    gatt_info_json TEXT DEFAULT '',  -- JSON: GATT Device Information Service (manufacturer, model, firmware)
    fast_pair_model TEXT DEFAULT ''  -- Google Fast Pair resolved device name (e.g. "Pixel Buds Pro")
);

CREATE TABLE observations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    mac TEXT NOT NULL,
    seen_at TEXT NOT NULL,          -- RFC3339 local datetime
    rssi INTEGER,                   -- signal strength in dBm (closer to 0 = stronger/nearer)
    name TEXT,
    service_uuids TEXT DEFAULT ''   -- Comma-separated
);

Indexes: idx_obs_mac(mac), idx_obs_seen(seen_at)

device_type values: phone, tablet, laptop, computer, watch, audio, speaker, tv, vehicle, smart_home, wearable, gaming, camera, printer, network, unknown

## Enrichment Columns

- continuity_json: JSON object with Apple Continuity protocol data. Has a "type" field (IBeacon, AirDrop, HomeKit, AirPods, AirPlay, Handoff, NearbyInfo, NearbyAction, AirPodsExtended, FindMy, Unknown). Use json_extract() to query fields, e.g.:
  - json_extract(continuity_json, '$.type') — continuity message type
  - json_extract(continuity_json, '$.battery_left') — AirPods left battery (0-10, multiply by 10 for %)
  - json_extract(continuity_json, '$.device_model') — AirPods model ID
- gatt_info_json: JSON object with GATT Device Information Service data. Fields: manufacturer_name, model_number, firmware_revision, hardware_revision, software_revision, probed_at. E.g.:
  - json_extract(gatt_info_json, '$.manufacturer_name')
  - json_extract(gatt_info_json, '$.model_number')
- fast_pair_model: Plain text device name from Google Fast Pair (e.g. "Pixel Buds Pro", "Galaxy Buds2 Pro")

## Timestamps

All timestamps are RFC3339 strings with timezone offset (e.g. "2025-06-15T14:30:00+01:00").
SQLite datetime/time functions work on these. Useful patterns:

- datetime('now', 'localtime') — current local time
- datetime(seen_at) — parse stored timestamp (for comparisons)
- time(seen_at) — extract HH:MM:SS for time-of-day queries
- date(seen_at) — extract YYYY-MM-DD for date queries
- strftime('%H', seen_at) — extract hour (00-23)
- strftime('%w', seen_at) — day of week (0=Sunday, 6=Saturday)
- julianday(a) - julianday(b) — difference in days (multiply by 24 for hours, 1440 for minutes)

Time-range filters:
- seen_at >= datetime('now', 'localtime', '-1 hour') — last hour
- seen_at >= datetime('now', 'localtime', '-7 days') — last week
- time(seen_at) BETWEEN '09:00' AND '17:00' — business hours
- date(first_seen) = date('now', 'localtime') — first seen today

## Observations Table

The observations table records every individual sighting of a device per scan cycle.
Use it for time-pattern analysis: when a device appears/disappears, visit frequency,
dwell time, signal strength over time, time-of-day patterns, and recurring schedules.
Join observations.mac = devices.mac to combine identity info with temporal data.

## Service UUID Names

Common UUIDs found in the service_uuids column (8-char hex prefix → name):
0000180a=Device Information, 0000180f=Battery, 0000180d=Heart Rate, 00001812=Human Interface Device,
0000110b=A2DP Sink, 0000110a=A2DP Source, 0000110d=Advanced Audio, 0000111e=Handsfree,
00001108=Headset, 0000184e=Audio Stream Control, 00001853=Common Audio, 00001203=Generic Audio,
0000fe9f=Google Fast Pair, 0000fe2c=Google Nearby, 0000feaa=Eddystone,
d0611e78=Apple Continuity, 7905f431=Apple ANCS, 89d3502b=Apple Media Service, 0000fd6f=Exposure Notification,
0000181a=Environmental Sensing, 0000fef5=Philips Hue, cba20d00=SwitchBot, 0000fee0=Xiaomi Mi Band,
0000feed=Tile, 0000febe=Bose, 00001800=Generic Access, 00001801=Generic Attribute,
00001814=Running Speed, 00001816=Cycling Speed, 00001819=Location & Navigation,
00001826=Fitness Machine, 0000fff6=Matter/Thread

Use these names when presenting service UUID data to the user.

## Instructions

Use the run_sql tool to query the database. Format responses as markdown with tables for tabular data. Be concise.
Only SELECT queries are allowed."#
    )
}

fn build_tools() -> Vec<ToolDef> {
    vec![ToolDef {
        r#type: "function",
        name: "run_sql",
        description: "Execute a read-only SQL SELECT query against the Bluetooth scan database. Returns JSON with {\"row_count\": N, \"rows\": [{column: value, ...}, ...]} on success, or {\"error\": \"message\"} on failure. Results are capped at 200 rows. Use SQLite syntax including datetime(), time(), date(), strftime(), julianday() for timestamp operations. Only SELECT statements are allowed.",
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "A SQLite SELECT query. Timestamps are RFC3339 strings (e.g. '2025-06-15T14:30:00+01:00'). Use datetime(), time(), strftime() for time operations. Use datetime('now','localtime') for current time."
                }
            },
            "required": ["query"],
            "additionalProperties": false
        }),
    }]
}

async fn run_chat_turn(
    api_key: &str,
    db_path: &str,
    user_text: &str,
    previous_response_id: Option<String>,
) -> ChatEvent {
    let client = reqwest::Client::new();
    let tools = build_tools();

    // First request: user message as input
    let mut input = serde_json::json!(user_text);
    let mut prev_id = previous_response_id;
    let mut iterations = 0;
    const MAX_ITERATIONS: usize = 10;

    loop {
        iterations += 1;
        if iterations > MAX_ITERATIONS {
            return ChatEvent::Error("Too many tool-call iterations, stopping.".into());
        }

        let body = ApiRequest {
            model: "gpt-4.1-nano",
            instructions: build_system_prompt(),
            input,
            tools: tools.clone(),
            previous_response_id: prev_id.clone(),
        };

        let resp = match client
            .post("https://api.openai.com/v1/responses")
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ChatEvent::Error(format!("Network error: {e}")),
        };

        let status = resp.status();
        let text = match resp.text().await {
            Ok(t) => t,
            Err(e) => return ChatEvent::Error(format!("Failed to read response: {e}")),
        };

        if !status.is_success() {
            return ChatEvent::Error(format!("API error ({status}): {text}"));
        }

        let api_resp: ApiResponse = match serde_json::from_str(&text) {
            Ok(r) => r,
            Err(e) => return ChatEvent::Error(format!("Failed to parse API response: {e}")),
        };

        if let Some(err) = &api_resp.error {
            return ChatEvent::Error(format!(
                "API error: {}",
                err.message.as_deref().unwrap_or("unknown error")
            ));
        }

        let response_id = api_resp.id.clone();

        // Check output items for function calls vs message
        let mut has_function_call = false;
        let mut tool_outputs: Vec<serde_json::Value> = Vec::new();

        for item in &api_resp.output {
            match item.r#type.as_deref() {
                Some("function_call") => {
                    has_function_call = true;
                    let call_id = item.call_id.as_deref().unwrap_or("");
                    let name = item.name.as_deref().unwrap_or("");

                    if name == "run_sql" {
                        let args_str = item.arguments.as_deref().unwrap_or("{}");
                        let result = match serde_json::from_str::<serde_json::Value>(args_str) {
                            Ok(args) => {
                                let query = args
                                    .get("query")
                                    .and_then(|q| q.as_str())
                                    .unwrap_or("");
                                execute_readonly_sql(db_path, query)
                            }
                            Err(e) => format!("{{\"error\": \"Invalid arguments: {e}\"}}")
                        };

                        tool_outputs.push(serde_json::json!({
                            "type": "function_call_output",
                            "call_id": call_id,
                            "output": result,
                        }));
                    }
                }
                Some("message") => {
                    // Extract text from content parts
                    if let Some(content) = &item.content {
                        let mut text_parts = Vec::new();
                        for part in content {
                            if part.r#type.as_deref() == Some("output_text") {
                                if let Some(t) = &part.text {
                                    text_parts.push(t.as_str());
                                }
                            }
                        }
                        let full_text = text_parts.join("");
                        if !full_text.is_empty() {
                            return ChatEvent::AssistantMessage {
                                text: full_text,
                                response_id,
                            };
                        }
                    }
                }
                _ => {}
            }
        }

        if has_function_call && !tool_outputs.is_empty() {
            // Send tool outputs back, continue loop
            input = serde_json::Value::Array(tool_outputs);
            prev_id = response_id;
            continue;
        }

        // No message and no function call — shouldn't happen, but handle it
        return ChatEvent::AssistantMessage {
            text: "(No response)".into(),
            response_id,
        };
    }
}

// ── SQL executor ────────────────────────────────────────────────────────

fn execute_readonly_sql(db_path: &str, query: &str) -> String {
    let trimmed = query.trim();

    // Only allow SELECT
    if !trimmed
        .split_whitespace()
        .next()
        .map_or(false, |w| w.eq_ignore_ascii_case("SELECT"))
    {
        return serde_json::json!({
            "error": "Only SELECT queries are allowed."
        })
        .to_string();
    }

    let conn = match Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(c) => c,
        Err(e) => {
            return serde_json::json!({ "error": format!("DB open error: {e}") }).to_string()
        }
    };

    let mut stmt = match conn.prepare(trimmed) {
        Ok(s) => s,
        Err(e) => {
            return serde_json::json!({ "error": format!("SQL error: {e}") }).to_string()
        }
    };

    let col_count = stmt.column_count();
    let col_names: Vec<String> = (0..col_count)
        .map(|i| stmt.column_name(i).unwrap_or("?").to_string())
        .collect();

    let mut rows = Vec::new();
    let result = stmt.query_map([], |row| {
        let mut obj = serde_json::Map::new();
        for (i, name) in col_names.iter().enumerate() {
            let val: rusqlite::Result<String> = row.get(i);
            let json_val = match val {
                Ok(s) => serde_json::Value::String(s),
                Err(_) => {
                    // Try as integer
                    if let Ok(n) = row.get::<_, i64>(i) {
                        serde_json::Value::Number(n.into())
                    } else if let Ok(f) = row.get::<_, f64>(i) {
                        serde_json::Number::from_f64(f)
                            .map(serde_json::Value::Number)
                            .unwrap_or(serde_json::Value::Null)
                    } else {
                        serde_json::Value::Null
                    }
                }
            };
            obj.insert(name.clone(), json_val);
        }
        Ok(serde_json::Value::Object(obj))
    });

    match result {
        Ok(mapped) => {
            for row_result in mapped {
                if rows.len() >= 200 {
                    break;
                }
                match row_result {
                    Ok(val) => rows.push(val),
                    Err(e) => {
                        return serde_json::json!({ "error": format!("Row error: {e}") })
                            .to_string()
                    }
                }
            }
        }
        Err(e) => {
            return serde_json::json!({ "error": format!("Query error: {e}") }).to_string()
        }
    }

    serde_json::json!({
        "row_count": rows.len(),
        "rows": rows,
    })
    .to_string()
}

// ── Markdown rendering ──────────────────────────────────────────────────

fn render_user_message(text: &str) -> Vec<Line<'static>> {
    vec![Line::from(vec![
        Span::styled(
            "You: ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(text.to_string(), Style::default().fg(Color::White)),
    ])]
}

fn render_system_message(text: &str) -> Vec<Line<'static>> {
    vec![Line::from(Span::styled(
        text.to_string(),
        Style::default().fg(Color::Red),
    ))]
}

fn render_assistant_message(text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        "Assistant:",
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    )));
    lines.extend(render_markdown(text));
    lines
}

pub fn render_markdown(text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut in_code_block = false;

    for line in text.lines() {
        if line.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }

        if in_code_block {
            lines.push(Line::from(Span::styled(
                format!("  {line}"),
                Style::default().fg(Color::Green).bg(Color::Rgb(30, 30, 30)),
            )));
            continue;
        }

        // Headers
        if line.starts_with("### ") {
            lines.push(Line::from(Span::styled(
                line[4..].to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if line.starts_with("## ") {
            lines.push(Line::from(Span::styled(
                line[3..].to_string(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if line.starts_with("# ") {
            lines.push(Line::from(Span::styled(
                line[2..].to_string(),
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }

        // Table separator rows (skip)
        if line.starts_with('|') && line.contains("---") {
            continue;
        }

        // Table rows
        if line.starts_with('|') {
            let cells: Vec<&str> = line
                .split('|')
                .filter(|s| !s.is_empty())
                .map(|s| s.trim())
                .collect();
            let mut spans = Vec::new();
            spans.push(Span::styled("│ ", Style::default().fg(Color::DarkGray)));
            for (i, cell) in cells.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
                }
                spans.extend(parse_inline(cell));
            }
            spans.push(Span::styled(" │", Style::default().fg(Color::DarkGray)));
            lines.push(Line::from(spans));
            continue;
        }

        // Bullet lists
        if line.starts_with("- ") || line.starts_with("* ") {
            let content = &line[2..];
            let mut spans = vec![Span::styled(
                "  • ",
                Style::default().fg(Color::Cyan),
            )];
            spans.extend(parse_inline(content));
            lines.push(Line::from(spans));
            continue;
        }

        // Regular text with inline formatting
        if line.is_empty() {
            lines.push(Line::from(""));
        } else {
            lines.push(Line::from(parse_inline(line)));
        }
    }

    lines
}

/// Parse inline markdown: **bold** and `code`
fn parse_inline(text: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        // Find the next special marker
        let bold_pos = remaining.find("**");
        let code_pos = remaining.find('`');

        let next = match (bold_pos, code_pos) {
            (Some(b), Some(c)) => {
                if b <= c {
                    Some(('*', b))
                } else {
                    Some(('`', c))
                }
            }
            (Some(b), None) => Some(('*', b)),
            (None, Some(c)) => Some(('`', c)),
            (None, None) => None,
        };

        match next {
            None => {
                spans.push(Span::raw(remaining.to_string()));
                break;
            }
            Some(('*', pos)) => {
                if pos > 0 {
                    spans.push(Span::raw(remaining[..pos].to_string()));
                }
                let after = &remaining[pos + 2..];
                if let Some(end) = after.find("**") {
                    spans.push(Span::styled(
                        after[..end].to_string(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ));
                    remaining = &after[end + 2..];
                } else {
                    spans.push(Span::raw("**".to_string()));
                    remaining = after;
                }
            }
            Some(('`', pos)) => {
                if pos > 0 {
                    spans.push(Span::raw(remaining[..pos].to_string()));
                }
                let after = &remaining[pos + 1..];
                if let Some(end) = after.find('`') {
                    spans.push(Span::styled(
                        after[..end].to_string(),
                        Style::default().fg(Color::Green),
                    ));
                    remaining = &after[end + 1..];
                } else {
                    spans.push(Span::raw("`".to_string()));
                    remaining = after;
                }
            }
            _ => unreachable!(),
        }
    }

    spans
}
