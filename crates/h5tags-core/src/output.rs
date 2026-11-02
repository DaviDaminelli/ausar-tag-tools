//! Tool results and their text and JSON renderings (PRD §7.1, §7.4, §8).
//! Text and JSON carry the same information.

use std::ops::Range;

use serde_json::{Map, Value, json};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// Aligned columns for people (CLI default in a terminal; decisions D23).
    Table,
    /// Tab-separated, what the AI reads (MCP default).
    Text,
    /// Structured data (CLI default when the output is piped or redirected).
    #[default]
    Json,
}

/// How table rows are laid out in the two text renderings.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// Tab-separated (`text`, for the AI: fewest tokens).
    Tabs,
    /// Space-padded, numbers right-aligned, the wide column last (`table`, for people).
    Aligned,
}

/// A table column: its header in text, its key in JSON.
#[derive(Clone, Copy, Debug)]
pub struct Column {
    pub header: &'static str,
    pub key: &'static str,
}

/// Column names, in one place (PRD §9a: every one is explained in AGENT_GUIDE.md).
pub mod col {
    use super::Column;
    pub const GLOBAL_ID: Column = Column { header: "global ID", key: "id" };
    pub const CLASS: Column = Column { header: "class", key: "class" };
    pub const CLASS_NAME: Column = Column { header: "class name", key: "class_name" };
    pub const DISPLAY_NAME: Column = Column { header: "display name", key: "name" };
    pub const NAME: Column = Column { header: "name", key: "name" };
    pub const COPIES: Column = Column { header: "copies", key: "copies" };
    pub const TAGS: Column = Column { header: "tags", key: "tags" };
    pub const DEFINITION: Column = Column { header: "definition", key: "definition" };
    pub const MODULE: Column = Column { header: "module", key: "module" };
    pub const VERSION: Column = Column { header: "version", key: "version" };
    pub const LOAD_MANIFEST: Column = Column { header: "load manifest", key: "load_manifest" };
    pub const STATUS: Column = Column { header: "status", key: "status" };
    pub const ENTRY_TYPE: Column = Column { header: "type", key: "type" };
    pub const KIND: Column = Column { header: "kind", key: "kind" };
    pub const SIZE: Column = Column { header: "size", key: "size" };
    pub const COMPRESSED: Column = Column { header: "compressed", key: "compressed" };
    pub const COUNT: Column = Column { header: "count", key: "count" };
    pub const TOTAL_SIZE: Column = Column { header: "total size", key: "total_size" };
    pub const FIELD_PATH: Column = Column { header: "field path", key: "path" };
    pub const FIELD_TYPE: Column = Column { header: "field type", key: "field_type" };
    pub const MATCHED: Column = Column { header: "matched", key: "matched" };

    pub const ALL: &[Column] = &[
        GLOBAL_ID,
        CLASS,
        CLASS_NAME,
        DISPLAY_NAME,
        NAME,
        COPIES,
        TAGS,
        DEFINITION,
        MODULE,
        VERSION,
        LOAD_MANIFEST,
        STATUS,
        ENTRY_TYPE,
        KIND,
        SIZE,
        COMPRESSED,
        COUNT,
        TOTAL_SIZE,
        FIELD_PATH,
        FIELD_TYPE,
        MATCHED,
    ];
}

/// State markers, in one place (PRD §9a: every one is explained in AGENT_GUIDE.md).
pub mod marker {
    pub const NOT_STORED: &str = "(not stored in this module)";
    pub const INCOMPLETE: &str = "!! incomplete file";
    pub const DAMAGED: &str = "!! damaged";
    pub const NOT_INDEXED: &str = "!! not indexed";
    // Field states (PRD §8).
    pub const EMPTY: &str = "(empty)";
    pub const NONE: &str = "(none)";
    pub const NONE_SET: &str = "(none set)";
    pub const NOT_IN_INDEX: &str = "(not in indexed modules)";
    pub const UNRESOLVED: &str = "(unresolved)";
    pub const NO_OPTIONS: &str = "(no options defined)";
    pub const NO_DEFINITION: &str = "(no definition for class";
    /// A struct or tag block whose stored type GUID isn't the definition's (PRD §6.4.5).
    pub const GUID_MISMATCH: &str = "definition GUID mismatch";

    pub const ALL: &[&str] = &[
        GUID_MISMATCH,
        NOT_STORED,
        INCOMPLETE,
        DAMAGED,
        NOT_INDEXED,
        EMPTY,
        NONE,
        NONE_SET,
        NOT_IN_INDEX,
        UNRESOLVED,
        NO_OPTIONS,
        NO_DEFINITION,
    ];
}

/// One table cell. Sizes render as `6.1 MB` in text and as bytes in JSON.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Text(String),
    Int(i64),
    /// Two decimals in text.
    Float(f64),
    Size(u64),
    Bool(bool),
    /// Named counts, most first: `mat 100, wcfg 31 (+3 more)` in text; in
    /// JSON `{ "items": [{ "name", "count" }], "more": 3 }`.
    Counts {
        items: Vec<(String, usize)>,
        more: usize,
    },
    /// Text with the parts that matched the query (byte ranges). The marks are
    /// highlighted in the colored `table` layout only; text and JSON show the text.
    Marked {
        text: String,
        marks: Vec<Range<usize>>,
    },
    /// No value for this row (empty in text, omitted in JSON).
    Empty,
}

impl Cell {
    pub fn marked(text: String, marks: Vec<Range<usize>>) -> Cell {
        Cell::Marked { text, marks }
    }

    fn marks(&self) -> &[Range<usize>] {
        match self {
            Cell::Marked { marks, .. } => marks,
            _ => &[],
        }
    }

    /// The first `max` of `counts`, with how many were left out.
    pub fn counts(counts: Vec<(String, usize)>, max: usize) -> Cell {
        let more = counts.len().saturating_sub(max);
        Cell::Counts { items: counts.into_iter().take(max).collect(), more }
    }

    fn text(&self) -> String {
        match self {
            Cell::Text(s) | Cell::Marked { text: s, .. } => s.clone(),
            Cell::Int(n) => n.to_string(),
            Cell::Float(x) => format!("{x:.2}"),
            Cell::Size(n) => format_size(*n),
            Cell::Bool(b) => if *b { "yes" } else { "no" }.into(),
            Cell::Counts { items, more } => {
                let list: Vec<String> = items.iter().map(|(name, n)| format!("{name} {n}")).collect();
                let more = if *more > 0 { format!(" (+{more} more)") } else { String::new() };
                format!("{}{more}", list.join(", "))
            }
            Cell::Empty => String::new(),
        }
    }

    fn json(&self) -> Option<Value> {
        match self {
            Cell::Text(s) | Cell::Marked { text: s, .. } => Some(json!(s)),
            Cell::Int(n) => Some(json!(n)),
            Cell::Float(x) => Some(json!(x)),
            Cell::Size(n) => Some(json!(n)),
            Cell::Bool(b) => Some(json!(b)),
            Cell::Counts { items, more } => {
                let items: Vec<Value> = items.iter().map(|(name, n)| json!({ "name": name, "count": n })).collect();
                Some(json!({ "items": items, "more": more }))
            }
            Cell::Empty => None,
        }
    }
}

impl From<String> for Cell {
    fn from(s: String) -> Cell {
        Cell::Text(s)
    }
}
impl From<std::borrow::Cow<'_, str>> for Cell {
    fn from(s: std::borrow::Cow<'_, str>) -> Cell {
        Cell::Text(s.into_owned())
    }
}
impl From<&str> for Cell {
    fn from(s: &str) -> Cell {
        Cell::Text(s.to_owned())
    }
}

/// A summary value shown above the table (`total: 12`).
#[derive(Clone, Debug)]
pub struct Summary {
    pub label: &'static str,
    pub key: &'static str,
    pub value: Cell,
}

#[derive(Clone, Debug, Default)]
pub struct Table {
    /// Set when more than one build is configured (PRD §6.1).
    pub build: Option<String>,
    pub total: usize,
    pub summary: Vec<Summary>,
    pub offset: usize,
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Cell>>,
    /// Index of a path column that may be shortened with `in <prefix>:` (text only).
    pub path_column: Option<usize>,
    /// The long name/path column: shown last in the `table` layout so the
    /// short columns line up (D23). Column order is unchanged in `text` and JSON.
    pub wide_column: Option<usize>,
    pub hints: Vec<String>,
}

/// One line of a report: `key: name value, name value` in text, `"key": {..}` in JSON.
#[derive(Clone, Debug)]
pub struct ReportItem {
    pub key: &'static str,
    pub values: Vec<(&'static str, Cell)>,
}

/// One titled table of a result made of several (`## dependencies` in text,
/// `"dependencies": {..}` in JSON).
#[derive(Clone, Debug)]
pub struct Part {
    pub title: &'static str,
    pub key: &'static str,
    pub table: Table,
}

#[derive(Clone, Debug)]
pub enum Output {
    Table(Table),
    /// A short report (e.g. `reindex`). Both renderings come from `items`.
    Report {
        build: Option<String>,
        items: Vec<ReportItem>,
    },
    /// Several titled tables (e.g. `header-dump`), in order.
    Parts {
        build: Option<String>,
        parts: Vec<Part>,
    },
    /// A tag or definition read field by field (`read_tag`, PRD §7.2–§7.7).
    Tag(crate::view::TagView),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ToolError {
    pub message: String,
    pub hint: Option<String>,
}

impl ToolError {
    pub fn new(message: impl Into<String>) -> ToolError {
        ToolError { message: message.into(), hint: None }
    }
    pub fn hint(mut self, hint: impl Into<String>) -> ToolError {
        self.hint = Some(hint.into());
        self
    }
}

/// `512 B`, `1.5 KB`, `6.1 MB`, `2.0 GB` (decimal units, decisions D6).
pub fn format_size(bytes: u64) -> String {
    const UNITS: [(&str, f64); 3] = [("GB", 1e9), ("MB", 1e6), ("KB", 1e3)];
    for (unit, scale) in UNITS {
        if bytes as f64 >= scale {
            return format!("{:.1} {unit}", bytes as f64 / scale);
        }
    }
    format!("{bytes} B")
}

/// Paging line (PRD §7.4).
fn more_line(remaining: usize, next: usize) -> String {
    format!("… {remaining} more (offset={next})")
}

/// Terminal colors for the `table` layout (decisions D24). Only applied when
/// the caller asks for color; text and JSON never contain them.
mod style {
    use anstyle::{AnsiColor, Style};
    pub const MATCH: Style = Style::new().bold().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Yellow)));
    pub const HEADER: Style = Style::new().bold();
    pub const ERROR: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Red)));
    pub const HINT: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Cyan)));
    pub const DIM: Style = Style::new().dimmed();
}

fn paint(text: &str, style: anstyle::Style, color: bool) -> String {
    if color && !text.is_empty() {
        format!("{}{text}{}", style.render(), style.render_reset())
    } else {
        text.to_owned()
    }
}

/// `text` with the byte ranges `marks` highlighted.
fn paint_marks(text: &str, marks: &[Range<usize>], color: bool) -> String {
    if !color || marks.is_empty() {
        return text.to_owned();
    }
    let mut out = String::new();
    let mut at = 0;
    for m in marks {
        // Skip marks that overlap the previous one or don't fit the text.
        let fits =
            m.start >= at && m.end <= text.len() && text.is_char_boundary(m.start) && text.is_char_boundary(m.end);
        if !fits {
            continue;
        }
        out.push_str(&text[at..m.start]);
        out.push_str(&paint(&text[m.clone()], style::MATCH, true));
        at = m.end;
    }
    out.push_str(&text[at..]);
    out
}

/// Space-pads `lines` (header first) into columns two spaces apart; `right`
/// columns are right-aligned; `last` is moved to the end and not padded.
/// With `color`, the header is bold, `!!` cells are red, and `last_marks`
/// (one entry per data line) are highlighted in the last column. Colors are
/// added after padding, so they never shift columns.
fn align(
    lines: &[Vec<String>],
    right: &[bool],
    last: Option<usize>,
    last_marks: &[Vec<Range<usize>>],
    color: bool,
) -> String {
    let count = right.len();
    let order: Vec<usize> = (0..count).filter(|&i| Some(i) != last).chain(last).collect();
    let widths: Vec<usize> =
        (0..count).map(|i| lines.iter().map(|l| l[i].chars().count()).max().unwrap_or(0)).collect();
    let mut out = String::new();
    for (n, line) in lines.iter().enumerate() {
        let cells: Vec<String> = order
            .iter()
            .enumerate()
            .map(|(k, &i)| {
                let is_last = k + 1 == order.len();
                let padded = match (is_last, right[i]) {
                    (true, _) => line[i].clone(),
                    (false, true) => format!("{:>w$}", line[i], w = widths[i]),
                    (false, false) => format!("{:<w$}", line[i], w = widths[i]),
                };
                if n == 0 {
                    paint(&padded, style::HEADER, color)
                } else if line[i].starts_with("!!") {
                    paint(&padded, style::ERROR, color)
                } else if is_last && Some(i) == last {
                    paint_marks(&padded, last_marks.get(n - 1).map_or(&[], |m| m), color)
                } else {
                    padded
                }
            })
            .collect();
        out.push_str(cells.join("  ").trim_end());
        out.push('\n');
    }
    out
}

/// Marks shifted left by `shift` bytes (after a prefix was cut); marks inside
/// the prefix are dropped, marks crossing it are clipped.
fn shift_marks(marks: &[Range<usize>], shift: usize) -> Vec<Range<usize>> {
    marks.iter().filter(|m| m.end > shift).map(|m| m.start.saturating_sub(shift)..m.end - shift).collect()
}

/// The longest folder prefix (ending in `\`) shared by every value.
fn common_folder(values: &[String]) -> Option<String> {
    let first = values.first()?;
    let mut len = first.len();
    for v in &values[1..] {
        len = first.bytes().zip(v.bytes()).take(len).take_while(|(a, b)| a.eq_ignore_ascii_case(b)).count();
    }
    // Cut after a `\`, which is always a character boundary.
    let cut = first.as_bytes()[..len].iter().rposition(|&b| b == b'\\')? + 1;
    Some(first[..cut].to_owned())
}

impl Table {
    fn remaining(&self) -> usize {
        self.total.saturating_sub(self.offset + self.rows.len())
    }

    /// Tab-separated rows: what the AI reads. Never colored.
    pub fn render_text(&self) -> String {
        self.render_lines(Layout::Tabs, false)
    }

    /// Aligned rows for people; same lines and content as [`Table::render_text`].
    /// `color` adds terminal colors (D24).
    pub fn render_table(&self, color: bool) -> String {
        self.render_lines(Layout::Aligned, color)
    }

    /// A column is right-aligned if every value in it is a number or a size.
    fn numeric_columns(&self) -> Vec<bool> {
        (0..self.columns.len())
            .map(|i| {
                let cells = || self.rows.iter().filter_map(|r| r.get(i)).filter(|c| **c != Cell::Empty);
                cells().next().is_some() && cells().all(|c| matches!(c, Cell::Int(_) | Cell::Float(_) | Cell::Size(_)))
            })
            .collect()
    }

    fn render_lines(&self, layout: Layout, color: bool) -> String {
        let mut out = String::new();
        if let Some(b) = &self.build {
            out.push_str(&format!("build: {b}\n"));
        }
        let mut summary = vec![format!("total: {}", self.total)];
        summary.extend(self.summary.iter().map(|s| format!("{}: {}", s.label, s.value.text())));
        out.push_str(&summary.join("  "));
        out.push('\n');

        let mut rows: Vec<Vec<String>> = self.rows.iter().map(|r| r.iter().map(Cell::text).collect()).collect();
        // Highlights of the wide column, kept in step with the prefix cut below.
        let mut wide_marks: Vec<Vec<Range<usize>>> = match self.wide_column {
            Some(w) => self.rows.iter().map(|r| r.get(w).map_or_else(Vec::new, |c| c.marks().to_vec())).collect(),
            None => Vec::new(),
        };
        if let Some(c) = self.path_column
            && rows.len() >= 2
        {
            let values: Vec<String> = rows.iter().map(|r| r[c].clone()).collect();
            if let Some(prefix) = common_folder(&values) {
                let cut = prefix.len();
                let prefix_marks: Vec<Range<usize>> = match self.wide_column {
                    Some(w) if w == c => self.rows[0][c]
                        .marks()
                        .iter()
                        .filter(|m| m.start < cut)
                        .map(|m| m.start..m.end.min(cut))
                        .collect(),
                    _ => Vec::new(),
                };
                out.push_str(&format!("in {}:\n", paint_marks(&prefix, &prefix_marks, color)));
                for r in &mut rows {
                    r[c] = r[c][cut..].to_owned();
                }
                if self.wide_column == Some(c) {
                    wide_marks = wide_marks.iter().map(|m| shift_marks(m, cut)).collect();
                }
            }
        }
        if !self.rows.is_empty() {
            let mut lines = vec![self.columns.iter().map(|c| c.header.to_owned()).collect::<Vec<_>>()];
            lines.extend(rows);
            match layout {
                Layout::Tabs => {
                    for line in lines {
                        out.push_str(&line.join("\t"));
                        out.push('\n');
                    }
                }
                Layout::Aligned => {
                    out.push_str(&align(&lines, &self.numeric_columns(), self.wide_column, &wide_marks, color))
                }
            }
        }
        if self.remaining() > 0 {
            out.push_str(&paint(&more_line(self.remaining(), self.offset + self.rows.len()), style::DIM, color));
            out.push('\n');
        }
        for h in &self.hints {
            out.push_str(&paint(&format!("hint: {h}"), style::HINT, color));
            out.push('\n');
        }
        out
    }

    pub fn render_json(&self) -> Value {
        let mut obj = Map::new();
        if let Some(b) = &self.build {
            obj.insert("build".into(), json!(b));
        }
        obj.insert("total".into(), json!(self.total));
        for s in &self.summary {
            if let Some(v) = s.value.json() {
                obj.insert(s.key.into(), v);
            }
        }
        obj.insert("offset".into(), json!(self.offset));
        let rows: Vec<Value> = self
            .rows
            .iter()
            .map(|r| {
                let mut row = Map::new();
                for (c, cell) in self.columns.iter().zip(r) {
                    if let Some(v) = cell.json() {
                        row.insert(c.key.into(), v);
                    }
                }
                Value::Object(row)
            })
            .collect();
        obj.insert("rows".into(), Value::Array(rows));
        if self.remaining() > 0 {
            obj.insert(
                "more".into(),
                json!({ "remaining": self.remaining(), "next_offset": self.offset + self.rows.len() }),
            );
        }
        if !self.hints.is_empty() {
            obj.insert("hints".into(), json!(self.hints));
        }
        Value::Object(obj)
    }
}

impl Output {
    /// Renders without colors (MCP, files, tests).
    pub fn render(&self, format: Format) -> String {
        self.render_with(format, false)
    }

    /// `color` applies to the `table` format only (D24).
    pub fn render_with(&self, format: Format, color: bool) -> String {
        match (self, format) {
            (Output::Table(t), Format::Text) => t.render_text(),
            (Output::Table(t), Format::Table) => t.render_table(color),
            (Output::Table(t), Format::Json) => pretty(&t.render_json()),
            (Output::Report { build, items }, Format::Text | Format::Table) => {
                let mut out = build.as_ref().map(|b| format!("build: {b}\n")).unwrap_or_default();
                for item in items {
                    let values: Vec<String> = item.values.iter().map(|(k, v)| format!("{k} {}", v.text())).collect();
                    out.push_str(&format!("{}: {}\n", item.key, values.join(", ")));
                }
                out
            }
            (Output::Report { build, items }, Format::Json) => {
                let mut obj = Map::new();
                if let Some(b) = build {
                    obj.insert("build".into(), json!(b));
                }
                for item in items {
                    let values: Map<String, Value> =
                        item.values.iter().filter_map(|(k, v)| v.json().map(|j| ((*k).to_owned(), j))).collect();
                    obj.insert(item.key.into(), Value::Object(values));
                }
                pretty(&Value::Object(obj))
            }
            (Output::Parts { build, parts }, Format::Text | Format::Table) => {
                let mut out = build.as_ref().map(|b| format!("build: {b}\n")).unwrap_or_default();
                for (i, p) in parts.iter().enumerate() {
                    if i > 0 {
                        out.push('\n');
                    }
                    out.push_str(&paint(&format!("## {}", p.title), style::HEADER, color));
                    out.push('\n');
                    out.push_str(&match format {
                        Format::Table => p.table.render_table(color),
                        _ => p.table.render_text(),
                    });
                }
                out
            }
            (Output::Tag(v), Format::Text) => v.render_lines(false),
            (Output::Tag(v), Format::Table) => v.render_lines(true),
            (Output::Tag(v), Format::Json) => pretty(&v.render_json()),
            (Output::Parts { build, parts }, Format::Json) => {
                let mut obj = Map::new();
                if let Some(b) = build {
                    obj.insert("build".into(), json!(b));
                }
                for p in parts {
                    obj.insert(p.key.into(), p.table.render_json());
                }
                pretty(&Value::Object(obj))
            }
        }
    }
}

impl ToolError {
    /// `!! error: <message>` + `hint: <next step>` (PRD §8), without colors.
    pub fn render(&self, format: Format) -> String {
        self.render_with(format, false)
    }

    /// `color` applies to the `table` format only (D24).
    pub fn render_with(&self, format: Format, color: bool) -> String {
        let color = color && format == Format::Table;
        match format {
            Format::Text | Format::Table => {
                let mut out = paint(&format!("!! error: {}", self.message), style::ERROR, color);
                out.push('\n');
                if let Some(h) = &self.hint {
                    out.push_str(&paint(&format!("hint: {h}"), style::HINT, color));
                    out.push('\n');
                }
                out
            }
            Format::Json => {
                let mut obj = Map::new();
                obj.insert("error".into(), json!(self.message));
                if let Some(h) = &self.hint {
                    obj.insert("hint".into(), json!(h));
                }
                pretty(&Value::Object(obj))
            }
        }
    }
}

fn pretty(v: &Value) -> String {
    let mut s = serde_json::to_string_pretty(v).expect("JSON values always serialize");
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(999), "999 B");
        assert_eq!(format_size(1500), "1.5 KB");
        assert_eq!(format_size(6_099_964), "6.1 MB");
        assert_eq!(format_size(2_000_000_000), "2.0 GB");
    }

    fn table() -> Table {
        Table {
            total: 5,
            offset: 0,
            columns: vec![col::GLOBAL_ID, col::DISPLAY_NAME],
            rows: vec![
                vec!["0x00000001".into(), r"objects\weapons\rifle\ar\ar.weapon".into()],
                vec!["0x00000002".into(), r"objects\weapons\rifle\br\br.weapon".into()],
            ],
            path_column: Some(1),
            ..Table::default()
        }
    }

    #[test]
    fn text_table_with_prefix_and_paging() {
        assert_eq!(
            table().render_text(),
            "total: 5\nin objects\\weapons\\rifle\\:\nglobal ID\tdisplay name\n0x00000001\tar\\ar.weapon\n0x00000002\tbr\\br.weapon\n… 3 more (offset=2)\n"
        );
    }

    #[test]
    fn json_table_has_same_information() {
        let v = table().render_json();
        assert_eq!(v["total"], 5);
        assert_eq!(v["rows"][1]["name"], r"objects\weapons\rifle\br\br.weapon");
        assert_eq!(v["more"]["remaining"], 3);
        assert_eq!(v["more"]["next_offset"], 2);
    }

    #[test]
    fn no_prefix_for_single_row_or_no_common_folder() {
        let mut t = table();
        t.rows[1][1] = r"levels\x.scenario".into();
        assert!(!t.render_text().contains("in "));
        t.rows.truncate(1);
        assert!(!t.render_text().contains("in "));
    }

    #[test]
    fn prefix_stops_at_folder_boundary() {
        let v = [r"a\bc\x".to_string(), r"a\bd\y".to_string()];
        assert_eq!(common_folder(&v).as_deref(), Some(r"a\"));
        assert_eq!(common_folder(&[r"ab".to_string(), r"ac".to_string()]), None);
    }

    #[test]
    fn errors() {
        let e = ToolError::new("unknown tag 'x'").hint("use search_tags");
        assert_eq!(e.render(Format::Text), "!! error: unknown tag 'x'\nhint: use search_tags\n");
        let v: Value = serde_json::from_str(&e.render(Format::Json)).unwrap();
        assert_eq!(v, json!({ "error": "unknown tag 'x'", "hint": "use search_tags" }));
    }

    /// PRD §9a: every column and marker a tool can output is explained in the guide.
    #[test]
    fn guide_explains_every_column_and_marker() {
        let guide = include_str!("../../../docs/AGENT_GUIDE.md");
        for c in col::ALL {
            assert!(
                guide.contains(&format!("| `{}` |", c.header)),
                "AGENT_GUIDE.md §4.7 doesn't explain column '{}'",
                c.header
            );
        }
        for m in marker::ALL {
            assert!(guide.contains(&format!("`{m}")), "AGENT_GUIDE.md doesn't explain marker '{m}'");
        }
    }

    #[test]
    fn table_layout_aligns_columns_and_puts_the_wide_one_last() {
        let t = Table {
            total: 3,
            columns: vec![col::GLOBAL_ID, col::CLASS, col::DISPLAY_NAME, col::COPIES],
            rows: vec![
                vec!["0x00003B5D".into(), "trak".into(), r"camera\warthog_g_camera.camera_track".into(), Cell::Int(1)],
                vec!["0x8551CDD8".into(), "effe".into(), r"fx\grinding\warthog\soft.effect".into(), Cell::Int(12)],
            ],
            wide_column: Some(2),
            ..Table::default()
        };
        assert_eq!(
            t.render_table(false),
            "total: 3\n\
             global ID   class  copies  display name\n\
             0x00003B5D  trak        1  camera\\warthog_g_camera.camera_track\n\
             0x8551CDD8  effe       12  fx\\grinding\\warthog\\soft.effect\n\
             … 1 more (offset=2)\n"
        );
        // The AI's text keeps the original column order and tabs.
        assert!(t.render_text().contains("global ID\tclass\tdisplay name\tcopies\n"));
    }

    fn painted(text: &str, s: anstyle::Style) -> String {
        format!("{}{text}{}", s.render(), s.render_reset())
    }

    /// Rows whose names share `objects\vehicles\` and match `warthog`.
    fn marked_table() -> Table {
        let row = |name: &str| {
            let marks: Vec<Range<usize>> = name.match_indices("warthog").map(|(i, m)| i..i + m.len()).collect();
            vec![Cell::marked(name.to_owned(), marks), Cell::Int(1)]
        };
        Table {
            total: 3,
            columns: vec![col::DISPLAY_NAME, col::COPIES],
            rows: vec![
                row(r"objects\vehicles\warthog\warthog.vehicle"),
                row(r"objects\vehicles\warthog\fx\horn.sound"),
            ],
            path_column: Some(0),
            wide_column: Some(0),
            hints: vec!["read this".into()],
            ..Table::default()
        }
    }

    #[test]
    fn colors_highlight_matches_after_the_prefix_cut() {
        let out = marked_table().render_table(true);
        // The shared prefix holds a match: highlighted on the `in` line.
        assert!(out.contains(&format!("in objects\\vehicles\\{}\\:\n", painted("warthog", style::MATCH))), "{out}");
        // Row matches are shifted with the cut.
        assert!(out.contains(&format!("{}.vehicle", painted("warthog", style::MATCH))), "{out}");
        assert!(
            out.contains(&format!("{}  {}", painted("copies", style::HEADER), painted("display name", style::HEADER))),
            "{out}"
        );
        assert!(out.contains(&painted("hint: read this", style::HINT)), "{out}");
        assert!(out.contains(&painted("… 1 more (offset=2)", style::DIM)), "{out}");
    }

    #[test]
    fn no_color_means_no_escape_codes() {
        let t = marked_table();
        assert!(!t.render_table(false).contains('\u{1b}'));
        assert!(!t.render_text().contains('\u{1b}'));
        assert!(!pretty(&t.render_json()).contains('\u{1b}'));
        // Same text with and without colors, once the codes are removed.
        let stripped: String = anstream::adapter::strip_str(&t.render_table(true)).to_string();
        assert_eq!(stripped, t.render_table(false));
    }

    #[test]
    fn errors_are_red_only_in_colored_tables() {
        let e = ToolError::new("no tag 'x'").hint("search_tags");
        assert_eq!(
            e.render_with(Format::Table, true),
            format!(
                "{}\n{}\n",
                painted("!! error: no tag 'x'", style::ERROR),
                painted("hint: search_tags", style::HINT)
            )
        );
        assert!(!e.render_with(Format::Text, true).contains('\u{1b}'));
    }

    fn parts() -> Output {
        let part = |title, key, name: &str| Part {
            title,
            key,
            table: Table { total: 1, columns: vec![col::NAME], rows: vec![vec![name.into()]], ..Table::default() },
        };
        Output::Parts {
            build: Some("forge".into()),
            parts: vec![part("header", "header", "a"), part("tag structs", "tag_structs", "b")],
        }
    }

    #[test]
    fn parts_are_titled_tables() {
        assert_eq!(
            parts().render(Format::Text),
            "build: forge
## header
total: 1
name
a

## tag structs
total: 1
name
b
"
        );
        assert!(parts().render_with(Format::Table, true).contains(&painted("## tag structs", style::HEADER)));
        let v: Value = serde_json::from_str(&parts().render(Format::Json)).unwrap();
        assert_eq!(v["build"], "forge");
        assert_eq!(v["tag_structs"]["rows"][0]["name"], "b");
        assert_eq!(v["header"]["total"], 1);
    }

    #[test]
    fn empty_cells_are_omitted_in_json() {
        let t = Table {
            total: 1,
            columns: vec![col::NAME, col::CLASS],
            rows: vec![vec!["folder\\".into(), Cell::Empty]],
            ..Table::default()
        };
        assert_eq!(t.render_json()["rows"][0], json!({ "name": "folder\\" }));
    }
}
