//! AI chat integration using the OpenAI Responses API.
//!
//! Provides a conversational interface for querying the Bluetooth scan database.
//! Includes SQL tool execution, markdown rendering for TUI display, and
//! conversation history management.

use crate::config::Config;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use tokio::sync::mpsc;

/// Available chat models, cycled via `m` key in chat mode.
const AVAILABLE_MODELS: &[&str] = &["gpt-5.4-mini", "gpt-5.4-nano", "gpt-5.4"];

const DEFAULT_SQL_MAX_ROWS: usize = 500;
const MAX_SQL_MAX_ROWS: usize = 5000;

// ── Data types ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
    System,
    /// Intermediate tool call status (dimmed, not an error).
    ToolStatus,
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub raw_text: String,
    pub rendered: Vec<Line<'static>>,
    /// Optional ID for in-place updates (tool status messages).
    pub status_id: Option<String>,
}

pub enum ChatEvent {
    AssistantMessage {
        text: String,
        history: Vec<serde_json::Value>,
    },
    /// Intermediate tool status. Messages with the same `id` update in place.
    Status { id: String, text: String },
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
    history: Vec<serde_json::Value>,
    api_key: Option<String>,
    db_path: String,
    model: String,
}

impl ChatState {
    pub fn new(db_path: String, cfg: &Config) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        // API key priority: DB (set via K key) → OPENAI_API_KEY env var
        let api_key = load_api_key_from_db(&db_path).or_else(|| {
            std::env::var("OPENAI_API_KEY")
                .ok()
                .filter(|k| !k.is_empty())
        });
        let model = cfg.openai_model.clone();
        Self {
            messages: Vec::new(),
            input: String::new(),
            scroll_offset: 0,
            waiting: false,
            tx,
            rx,
            history: Vec::new(),
            api_key,
            db_path,
            model,
        }
    }

    pub fn set_api_key(&mut self, key: Option<String>) {
        self.api_key = key.filter(|k| !k.is_empty());
    }

    /// Current model name for display.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Cycle to the next available model.
    pub fn cycle_model(&mut self) {
        let current_idx = AVAILABLE_MODELS
            .iter()
            .position(|&m| m == self.model)
            .unwrap_or(0);
        let next_idx = (current_idx + 1) % AVAILABLE_MODELS.len();
        self.model = AVAILABLE_MODELS[next_idx].to_string();
    }

    fn push_message(&mut self, role: ChatRole, text: String) {
        let rendered = match role {
            ChatRole::User => render_user_message(&text),
            ChatRole::Assistant => render_assistant_message(&text),
            ChatRole::System => render_system_message(&text),
            ChatRole::ToolStatus => render_tool_status(&text),
        };
        self.messages.push(ChatMessage {
            role,
            raw_text: text,
            rendered,
            status_id: None,
        });
    }

    pub fn send_message(&mut self) {
        if self.waiting || self.input.trim().is_empty() {
            return;
        }

        let api_key = match &self.api_key {
            Some(k) => k.clone(),
            None => {
                self.push_message(
                    ChatRole::System,
                    "Press K to set an OpenAI API key in the TUI.".into(),
                );
                return;
            }
        };

        let text = std::mem::take(&mut self.input);
        self.push_message(ChatRole::User, text.clone());

        self.waiting = true;
        self.scroll_offset = 0;

        let tx = self.tx.clone();
        let db_path = self.db_path.clone();
        let history = self.history.clone();
        let model = self.model.clone();

        tokio::spawn(async move {
            let event =
                run_chat_turn(&api_key, &model, &db_path, &text, history, &tx).await;
            let _ = tx.send(event);
        });
    }

    pub fn drain_events(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                ChatEvent::AssistantMessage { text, history } => {
                    self.history = history;
                    self.push_message(ChatRole::Assistant, text);
                    self.waiting = false;
                    self.scroll_offset = 0;
                }
                ChatEvent::Status { id, text } => {
                    let rendered = render_tool_status(&text);
                    // Update existing message with same id, or add new one
                    if let Some(existing) = self
                        .messages
                        .iter_mut()
                        .rev()
                        .find(|m| m.status_id.as_deref() == Some(&id))
                    {
                        existing.raw_text = text;
                        existing.rendered = rendered;
                    } else {
                        self.messages.push(ChatMessage {
                            role: ChatRole::ToolStatus,
                            raw_text: text,
                            rendered,
                            status_id: Some(id),
                        });
                    }
                }
                ChatEvent::Error(err) => {
                    self.push_message(ChatRole::System, err);
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

    #[allow(dead_code)]
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    /// Total rendered lines across all messages (including spacing).
    #[allow(dead_code)]
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

fn load_api_key_from_db(db_path: &str) -> Option<String> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let mut stmt = conn
        .prepare("SELECT value FROM app_settings WHERE key = 'openai_api_key'")
        .ok()?;
    let mut rows = stmt.query([]).ok()?;
    let row = rows.next().ok()??;
    let value: String = row.get(0).ok()?;
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

// ── OpenAI Responses API types ──────────────────────────────────────────

#[derive(Deserialize)]
struct ApiResponse {
    #[allow(dead_code)]
    id: Option<String>,
    #[serde(default)]
    output: Vec<serde_json::Value>,
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

fn user_message_item(text: &str) -> serde_json::Value {
    serde_json::json!({
        "role": "user",
        "content": text
    })
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
    service_uuids TEXT DEFAULT '',  -- Comma-separated
    fingerprint TEXT DEFAULT ''     -- fingerprint at observation time
);

Indexes: idx_obs_mac(mac), idx_obs_seen(seen_at)

device_type values: phone, tablet, laptop, computer, watch, audio, speaker, tv, vehicle, smart, wearable, gaming, camera, printer, network, unknown

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

Use `run_sql` for database access. Use `code_interpreter` for multi-step analysis, anomaly scoring, or summarizing large SQL results. Use `web_search` only when external current facts are necessary.
Prefer SQL aggregation over dumping raw rows. If you need CTEs, `WITH ... SELECT ...` queries are allowed.

**Formatting**: Responses are rendered in a terminal. Keep markdown tables narrow — abbreviate column headers, truncate long values, and limit to ~5 columns so they fit in ~80 chars. Use bullet lists instead of tables when there are only 1-2 columns. Be concise and cite web sources when you use web search."#
    )
}

fn build_tools() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "type": "function",
            "name": "run_sql",
            "description": "Execute a read-only SQLite query against the Bluetooth scan database. Returns JSON with row_count, rows, truncated, and max_rows on success, or an error field on failure. Only SELECT queries and WITH...SELECT queries are allowed.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "A single SQLite SELECT query. CTEs using WITH are allowed. Timestamps are RFC3339 strings with timezone offsets."
                    },
                    "max_rows": {
                        "type": "integer",
                        "description": "Optional maximum rows to return. Defaults to 500 and is capped at 5000.",
                        "minimum": 1,
                        "maximum": 5000
                    }
                },
                "required": ["query"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "type": "code_interpreter",
            "container": {
                "type": "auto",
                "memory_limit": "4g"
            }
        }),
        serde_json::json!({
            "type": "web_search"
        }),
    ]
}

async fn run_chat_turn(
    api_key: &str,
    model: &str,
    db_path: &str,
    user_text: &str,
    history: Vec<serde_json::Value>,
    tx: &mpsc::UnboundedSender<ChatEvent>,
) -> ChatEvent {
    let client = reqwest::Client::new();
    let tools = build_tools();

    let mut context = history;
    context.push(user_message_item(user_text));
    let mut iterations = 0;
    const MAX_ITERATIONS: usize = 10;

    loop {
        iterations += 1;
        if iterations > MAX_ITERATIONS {
            return ChatEvent::Error("Too many tool-call iterations, stopping.".into());
        }

        let body = serde_json::json!({
            "model": model,
            "instructions": build_system_prompt(),
            "input": context,
            "tools": tools,
            "store": false,
            "include": ["reasoning.encrypted_content"],
            "parallel_tool_calls": true,
            "reasoning": { "effort": "medium" },
            "text": { "verbosity": "low" }
        });

        let api_resp = match create_response(&client, api_key, &body).await {
            Ok(resp) => resp,
            Err(err) => return ChatEvent::Error(err),
        };

        if let Some(err) = &api_resp.error {
            return ChatEvent::Error(format!(
                "API error: {}",
                err.message.as_deref().unwrap_or("unknown error")
            ));
        }

        let mut function_calls = Vec::new();
        let mut text_parts: Vec<String> = Vec::new();
        let mut tool_outputs: Vec<serde_json::Value> = Vec::new();

        for raw_item in &api_resp.output {
            let Ok(item) = serde_json::from_value::<OutputItem>(raw_item.clone()) else {
                continue;
            };
            match item.r#type.as_deref() {
                Some("function_call") => {
                    function_calls.push(item);
                }
                Some("message") => {
                    if let Some(content) = &item.content {
                        for part in content {
                            if part.r#type.as_deref() == Some("output_text") {
                                if let Some(t) = &part.text {
                                    text_parts.push(t.clone());
                                }
                            }
                        }
                    }
                }
                _ => {
                    // Surface server-side tool usage (web_search, code_interpreter)
                    if let Some(t) = item.r#type.as_deref() {
                        let (id, msg) = match t {
                            "web_search_call" => {
                                (Some(t.to_string()), "\u{25D0} Searching the web...")
                            }
                            "code_interpreter_call" => {
                                (Some(t.to_string()), "\u{25D0} Running code analysis...")
                            }
                            _ => (None, ""),
                        };
                        if let Some(id) = id {
                            let _ = tx.send(ChatEvent::Status {
                                id,
                                text: msg.to_string(),
                            });
                        }
                    }
                }
            }
        }

        if !function_calls.is_empty() {
            context.extend(api_resp.output.iter().cloned());

            for item in &function_calls {
                let call_id = item.call_id.as_deref().unwrap_or("");
                let name = item.name.as_deref().unwrap_or("");

                if name == "run_sql" {
                    let args_str = item.arguments.as_deref().unwrap_or("{}");
                    let parsed = serde_json::from_str::<serde_json::Value>(args_str);
                    let query_preview = parsed
                        .as_ref()
                        .ok()
                        .and_then(|v| v.get("query").and_then(|q| q.as_str()))
                        .unwrap_or("")
                        .to_string();
                    let short = if query_preview.len() > 72 {
                        format!("{}...", &query_preview[..69])
                    } else {
                        query_preview
                    };

                    let status_id = call_id.to_string();
                    let _ = tx.send(ChatEvent::Status {
                        id: status_id.clone(),
                        text: format!("\u{25D0} SQL: {short}"),
                    });

                    let result = match parsed {
                        Ok(args) => {
                            let query = args
                                .get("query")
                                .and_then(|q| q.as_str())
                                .unwrap_or("");
                            let max_rows = args
                                .get("max_rows")
                                .and_then(|v| v.as_u64())
                                .map(|v| v as usize);
                            execute_readonly_sql(db_path, query, max_rows)
                        }
                        Err(e) => format!("{{\"error\": \"Invalid arguments: {e}\"}}")
                    };

                    // Update the same status line with completion
                    let row_info = serde_json::from_str::<serde_json::Value>(&result)
                        .ok()
                        .and_then(|v| v.get("row_count").and_then(|n| n.as_u64()))
                        .map(|n| format!(" ({n} rows)"))
                        .unwrap_or_default();
                    let _ = tx.send(ChatEvent::Status {
                        id: status_id,
                        text: format!("\u{2713} SQL{row_info}: {short}"),
                    });

                    tool_outputs.push(serde_json::json!({
                        "type": "function_call_output",
                        "call_id": call_id,
                        "output": result,
                    }));
                } else {
                    let _ = tx.send(ChatEvent::Status {
                        id: call_id.to_string(),
                        text: format!("\u{25D0} Using tool: {name}"),
                    });
                }
            }

            context.extend(tool_outputs);
            continue;
        }

        let full_text = text_parts.join("");
        context.extend(api_resp.output.iter().cloned());
        if !full_text.is_empty() {
            return ChatEvent::AssistantMessage {
                text: full_text,
                history: context,
            };
        }

        return ChatEvent::AssistantMessage {
            text: "(No response)".into(),
            history: context,
        };
    }
}

async fn create_response(
    client: &reqwest::Client,
    api_key: &str,
    body: &serde_json::Value,
) -> Result<ApiResponse, String> {
    let resp = client
        .post("https://api.openai.com/v1/responses")
        .bearer_auth(api_key)
        .json(body)
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {e}"))?;

    if !status.is_success() {
        return Err(format!("API error ({status}): {text}"));
    }

    serde_json::from_str(&text).map_err(|e| format!("Failed to parse API response: {e}"))
}

// ── SQL executor ────────────────────────────────────────────────────────

fn error_json(msg: impl std::fmt::Display) -> String {
    serde_json::json!({ "error": msg.to_string() }).to_string()
}

fn execute_readonly_sql(db_path: &str, query: &str, max_rows: Option<usize>) -> String {
    let trimmed = query.trim();

    let first_word = trimmed
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let is_allowed = matches!(first_word.as_str(), "SELECT" | "WITH");
    if !is_allowed || trimmed.contains(';') {
        return error_json("Only single SELECT queries or WITH...SELECT queries are allowed.");
    }

    let max_rows = max_rows
        .unwrap_or(DEFAULT_SQL_MAX_ROWS)
        .max(1)
        .min(MAX_SQL_MAX_ROWS);

    let conn = match Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(c) => c,
        Err(e) => {
            return error_json(format!("DB open error: {e}"))
        }
    };

    let mut stmt = match conn.prepare(trimmed) {
        Ok(s) => s,
        Err(e) => {
            return error_json(format!("SQL error: {e}"))
        }
    };

    let col_count = stmt.column_count();
    let col_names: Vec<String> = (0..col_count)
        .map(|i| stmt.column_name(i).unwrap_or("?").to_string())
        .collect();

    let mut rows = Vec::new();
    let mut truncated = false;
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
                if rows.len() >= max_rows {
                    truncated = true;
                    break;
                }
                match row_result {
                    Ok(val) => rows.push(val),
                    Err(e) => return error_json(format!("Row error: {e}")),
                }
            }
        }
        Err(e) => {
            return error_json(format!("Query error: {e}"))
        }
    }

    serde_json::json!({
        "row_count": rows.len(),
        "rows": rows,
        "truncated": truncated,
        "max_rows": max_rows,
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

fn render_tool_status(text: &str) -> Vec<Line<'static>> {
    // ◐ (spinner) is yellow, ✓ (done) is green, rest is dim
    let (icon, rest) = text.split_at(text.find(' ').unwrap_or(text.len()));
    let icon_color = if icon.contains('\u{2713}') {
        Color::Green
    } else {
        Color::Yellow
    };
    vec![Line::from(vec![
        Span::styled(icon.to_string(), Style::default().fg(icon_color)),
        Span::styled(rest.to_string(), Style::default().fg(Color::DarkGray)),
    ])]
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

        // Headers (check longest prefix first)
        if let Some(heading) = line
            .strip_prefix("### ")
            .map(|t| render_heading(t, Color::Cyan))
            .or_else(|| line.strip_prefix("## ").map(|t| render_heading(t, Color::Yellow)))
            .or_else(|| line.strip_prefix("# ").map(|t| render_heading(t, Color::Magenta)))
        {
            lines.push(heading);
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

fn render_heading(text: &str, color: Color) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    ))
}

/// Parse inline markdown: **bold** and `code`.
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

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: extract all raw text from a vec of Spans
    fn spans_text(spans: &[Span]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn lines_text(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|l| spans_text(&l.spans)).collect()
    }

    // ── render_markdown ──────────────────────────────────────────────────

    #[test]
    fn render_h1() {
        let lines = render_markdown("# Title");
        assert_eq!(lines.len(), 1);
        assert_eq!(spans_text(&lines[0].spans), "Title");
    }

    #[test]
    fn render_h2() {
        let lines = render_markdown("## Subtitle");
        assert_eq!(spans_text(&lines[0].spans), "Subtitle");
    }

    #[test]
    fn render_h3() {
        let lines = render_markdown("### Section");
        assert_eq!(spans_text(&lines[0].spans), "Section");
    }

    #[test]
    fn render_bullet_list() {
        let lines = render_markdown("- item one\n- item two");
        assert_eq!(lines.len(), 2);
        let text = lines_text(&lines);
        assert!(text[0].contains("item one"));
        assert!(text[1].contains("item two"));
    }

    #[test]
    fn render_code_block() {
        let md = "```\nlet x = 1;\n```";
        let lines = render_markdown(md);
        assert_eq!(lines.len(), 1); // only the code line, ``` delimiters are stripped
        assert!(spans_text(&lines[0].spans).contains("let x = 1;"));
    }

    #[test]
    fn render_table_row() {
        let md = "| Name | Value |\n|---|---|\n| foo | bar |";
        let lines = render_markdown(md);
        // Header row + data row (separator skipped)
        assert_eq!(lines.len(), 2);
        let data_row = spans_text(&lines[1].spans);
        assert!(data_row.contains("foo"));
        assert!(data_row.contains("bar"));
    }

    #[test]
    fn render_empty_line() {
        let lines = render_markdown("before\n\nafter");
        assert_eq!(lines.len(), 3);
        assert_eq!(spans_text(&lines[1].spans), "");
    }

    // ── parse_inline ─────────────────────────────────────────────────────

    #[test]
    fn inline_plain_text() {
        let spans = parse_inline("hello world");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.as_ref(), "hello world");
    }

    #[test]
    fn inline_bold() {
        let spans = parse_inline("this is **bold** text");
        assert_eq!(spans_text(&spans), "this is bold text");
        // The bold span should have BOLD modifier
        assert!(spans[1].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn inline_code() {
        let spans = parse_inline("use `code` here");
        assert_eq!(spans_text(&spans), "use code here");
        // The code span should have green color
        assert_eq!(spans[1].style.fg, Some(Color::Green));
    }

    #[test]
    fn inline_mixed() {
        let spans = parse_inline("**bold** and `code`");
        assert_eq!(spans_text(&spans), "bold and code");
    }

    #[test]
    fn inline_unclosed_bold() {
        let spans = parse_inline("open **bold");
        let text = spans_text(&spans);
        assert!(text.contains("**"));
        assert!(text.contains("bold"));
    }

    #[test]
    fn inline_unclosed_code() {
        let spans = parse_inline("open `code");
        let text = spans_text(&spans);
        assert!(text.contains("`"));
        assert!(text.contains("code"));
    }

    // ── execute_readonly_sql ─────────────────────────────────────────────

    #[test]
    fn sql_valid_select() {
        let tmp = std::env::temp_dir().join("bluemon_test_chat.db");
        let tmp_path = tmp.to_str().unwrap();
        let _conn = crate::db::open(tmp_path).unwrap();

        let result = execute_readonly_sql(tmp_path, "SELECT COUNT(*) as cnt FROM devices", None);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["row_count"], 1);

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn sql_rejects_non_select() {
        let tmp = std::env::temp_dir().join("bluemon_test_chat_reject.db");
        let tmp_path = tmp.to_str().unwrap();
        let _conn = crate::db::open(tmp_path).unwrap();

        let result = execute_readonly_sql(tmp_path, "DROP TABLE devices", None);
        assert!(result.contains("error"));
        assert!(result.contains("Only single SELECT queries"));

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn sql_malformed_query() {
        let tmp = std::env::temp_dir().join("bluemon_test_chat_malformed.db");
        let tmp_path = tmp.to_str().unwrap();
        let _conn = crate::db::open(tmp_path).unwrap();

        let result = execute_readonly_sql(tmp_path, "SELECT * FROM nonexistent_table", None);
        assert!(result.contains("error"));

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn sql_row_cap() {
        let tmp = std::env::temp_dir().join("bluemon_test_chat_cap.db");
        let tmp_path = tmp.to_str().unwrap();
        let _ = std::fs::remove_file(&tmp);
        let conn = crate::db::open(tmp_path).unwrap();

        // Insert 210 devices
        for i in 0..210 {
            conn.execute(
                "INSERT INTO devices (mac, device_type, first_seen, last_seen, sightings) VALUES (?1, 'unknown', '2025-01-01T00:00:00Z', '2025-01-01T00:00:00Z', 1)",
                rusqlite::params![format!("MAC{i:04}")],
            ).unwrap();
        }
        drop(conn);

        let result = execute_readonly_sql(tmp_path, "SELECT * FROM devices", None);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["row_count"], 210);
        assert_eq!(parsed["max_rows"], super::DEFAULT_SQL_MAX_ROWS);
        assert_eq!(parsed["truncated"], false);

        let _ = std::fs::remove_file(&tmp);
    }
}
