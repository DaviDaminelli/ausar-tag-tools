//! A tag or definition as the AI and people read it (PRD §7.2–§7.7): one tree
//! of nodes, rendered as `text`, `table` or JSON. Every rendering comes from
//! the same nodes (PRD §13). `text` separates columns with two spaces;
//! `table` aligns them (decisions D36).

use serde_json::{Map, Value, json};

/// What a heading is (PRD §7.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadingKind {
    /// `## OBJECT`: a top-level section label.
    ClassSection,
    /// `### <heading>` + `> text`.
    Explanation,
    /// A `_field_custom` group heading.
    Group,
    /// What `read_tag` was asked for: `### barrels[1]  (path)`, `### EXPOSURE  (section)`.
    Target,
}

impl HeadingKind {
    fn key(self) -> &'static str {
        match self {
            HeadingKind::ClassSection => "class_section",
            HeadingKind::Explanation => "explanation",
            HeadingKind::Group => "group",
            HeadingKind::Target => "target",
        }
    }
}

/// A §8 state: the marker in text, the `state` in JSON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    /// What text shows, e.g. `(empty)`, `(no option 7; 4 defined)`.
    pub text: String,
    /// JSON `state` (`empty`, `none`, `not_found`, `no_option`, …).
    pub key: &'static str,
    /// A tool failure (`!!`): JSON puts it under `error`, not `state`.
    pub error: bool,
}

/// A field's value: its text, and the JSON keys it adds (`value`, `label`,
/// `raw`, `set`, `target`, …).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shown {
    pub text: String,
    pub json: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ViewNode {
    Heading {
        kind: HeadingKind,
        title: String,
        /// Explanation text: the first line at `normal` (with ` […]` if cut),
        /// all of it at `full`.
        text: Option<String>,
    },
    Field(Box<FieldView>),
}

/// One field line, or a struct / array / tag block with its children.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FieldView {
    pub name: String,
    pub path: String,
    /// The XML type name (`_field_short_enum`), JSON `type`.
    pub xml_type: &'static str,
    /// The short type of AGENT_GUIDE §4.3 (`enum16`, `block[2]`).
    pub short_type: String,
    pub value: Option<Shown>,
    pub units: Option<String>,
    pub range: Option<String>,
    /// Options as text shows them (`{a|b|c}`, `{a, b}  (+3 options)`) and JSON lists them.
    pub options: Option<(String, Vec<String>)>,
    /// The options are in the view's `legend:` instead (`[*]` reads, AGENT_GUIDE §4.4).
    pub in_legend: bool,
    pub state: Option<State>,
    pub warning: Option<String>,
    pub hidden: bool,
    /// `detail=full` only.
    pub help: Option<String>,
    pub markers: Option<String>,
    pub offset: Option<u32>,
    /// Set for structs, arrays and tag blocks.
    pub container: Option<Container>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Container {
    /// After the short type in the heading: `{primary, secondary}  showing [0] primary`.
    pub summary: Option<String>,
    /// For tag blocks: entry count, labels, the entry shown (JSON `count`, `labels`, `index`).
    pub count: Option<u32>,
    pub labels: Vec<String>,
    pub index: Option<u32>,
    /// Not expanded because of `depth`: `→ path=<path>` (PRD §7.4).
    pub stub: bool,
    pub children: Vec<ViewNode>,
    /// Fields after a read failure, listed at `detail=full` as `(not read)`.
    pub not_read: Vec<String>,
}

/// The tag line (PRD §7.3) and the tree below it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TagView {
    /// `# <class code> <display name>  <global ID>  build=<b>  copies: …  resources: …`.
    pub title: String,
    /// The same facts for JSON (`class`, `name`, `id`, `build`, `copies`, `resources`).
    pub title_json: Map<String, Value>,
    /// Extra lines below the title (`detail=full`: one per resource).
    pub title_lines: Vec<String>,
    pub nodes: Vec<ViewNode>,
    /// `detail=brief`: `<full path>  <value>` lines instead of the tree.
    pub brief: bool,
    /// `[*]` reads: each field's options once, after the fields (AGENT_GUIDE §4.4).
    pub legend: Vec<String>,
    pub hints: Vec<String>,
}

/// The columns of one field line, before joining.
fn columns(f: &FieldView) -> Vec<String> {
    let mut cols = vec![f.name.clone(), f.short_type.clone()];
    let mut value = f.value.as_ref().map(|v| v.text.clone()).unwrap_or_default();
    if let Some(s) = f.state.as_ref().filter(|s| !s.text.is_empty()) {
        value = if value.is_empty() { s.text.clone() } else { format!("{value} {}", s.text) };
    }
    cols.push(value);
    let units = match (&f.units, &f.range) {
        (Some(u), Some(r)) => format!("[{r}] {u}"),
        (Some(u), None) => u.clone(),
        (None, Some(r)) => format!("[{r}]"),
        (None, None) => String::new(),
    };
    cols.push(units);
    if let Some((text, _)) = f.options.as_ref().filter(|_| !f.in_legend) {
        cols.push(text.clone());
    }
    if f.hidden {
        cols.push("(hidden)".into());
    }
    if let Some(w) = &f.warning {
        cols.push(format!("warning: {w}"));
    }
    if let Some(m) = &f.markers {
        cols.push(format!("markers {m}"));
    }
    if let Some(o) = f.offset {
        cols.push(format!("@0x{o:X}"));
    }
    if let Some(h) = &f.help {
        cols.push(format!("# {h}"));
    }
    // Name and type always; the value and the optional columns only when they
    // say something (a definition's fields have no value).
    let optional = cols.split_off(2);
    cols.extend(optional.into_iter().filter(|c| !c.is_empty()));
    while cols.last().is_some_and(|c| c.is_empty()) {
        cols.pop();
    }
    cols
}

/// Writes runs of field lines: `aligned` pads the name and type columns.
struct Lines {
    out: String,
    aligned: bool,
    /// Field lines waiting for their column widths: (indent, columns).
    run: Vec<(usize, Vec<String>)>,
}

impl Lines {
    fn flush(&mut self) {
        let widths: Vec<usize> = (0..2)
            .map(|c| self.run.iter().map(|(_, cols)| cols.get(c).map_or(0, |s| s.chars().count())).max().unwrap_or(0))
            .collect();
        for (indent, cols) in self.run.drain(..) {
            let mut line = " ".repeat(indent);
            for (i, c) in cols.iter().enumerate() {
                if i > 0 {
                    line.push_str("  ");
                }
                line.push_str(c);
                if self.aligned && i < 2 && i + 1 < cols.len() {
                    line.push_str(&" ".repeat(widths[i].saturating_sub(c.chars().count())));
                }
            }
            self.out.push_str(line.trim_end());
            self.out.push('\n');
        }
    }

    fn line(&mut self, indent: usize, text: &str) {
        self.flush();
        self.out.push_str(&" ".repeat(indent));
        self.out.push_str(text);
        self.out.push('\n');
    }
}

fn heading_line(level: usize, f: &FieldView) -> String {
    let mut s = format!("{} {}  {}", "#".repeat(level), f.name, f.short_type);
    if let Some(c) = &f.container {
        if let Some(summary) = &c.summary {
            s.push_str("  ");
            s.push_str(summary);
        }
        if c.stub {
            s.push_str(&format!("  → path={}", f.path));
        }
    }
    if let Some(st) = &f.state {
        s.push_str("  ");
        s.push_str(&st.text);
    }
    if let Some(w) = &f.warning {
        s.push_str(&format!("  warning: {w}"));
    }
    if f.hidden {
        s.push_str("  (hidden)");
    }
    s
}

fn write_nodes(nodes: &[ViewNode], depth: usize, lines: &mut Lines) {
    let indent = depth * 2;
    for n in nodes {
        match n {
            ViewNode::Heading { kind, title, text } => {
                if *kind == HeadingKind::ClassSection && depth == 0 {
                    lines.line(0, &format!("## {title}"));
                } else if !title.is_empty() {
                    lines.line(indent, &format!("{} {title}", "#".repeat(3 + depth)));
                }
                if let Some(t) = text {
                    for l in t.lines() {
                        lines.line(indent, &format!("> {l}"));
                    }
                }
            }
            ViewNode::Field(f) => match &f.container {
                // An empty block or a container shown on one line keeps the field-line form.
                Some(c) if !c.stub && (!c.children.is_empty() || f.state.as_ref().is_some_and(|s| s.error)) => {
                    lines.line(indent, &heading_line(3 + depth, f));
                    write_nodes(&c.children, depth + 1, lines);
                    for name in &c.not_read {
                        lines.line(indent + 2, &format!("{name}  (not read)"));
                    }
                }
                Some(_) if f.state.as_ref().is_some_and(|s| s.key == "empty") => {
                    lines.run.push((indent, columns(f)));
                }
                Some(_) => lines.line(indent, &heading_line(3 + depth, f)),
                None => lines.run.push((indent, columns(f))),
            },
        }
    }
    lines.flush();
}

fn write_brief(nodes: &[ViewNode], out: &mut String) {
    for n in nodes {
        let ViewNode::Field(f) = n else { continue };
        match &f.container {
            Some(c) if !c.children.is_empty() => write_brief(&c.children, out),
            _ => {
                let cols = columns(f);
                let value = cols.get(2).filter(|v| !v.is_empty()).cloned().unwrap_or_else(|| f.short_type.clone());
                out.push_str(&format!("{}  {value}\n", f.path));
            }
        }
    }
}

fn field_json(f: &FieldView) -> Value {
    let mut o = Map::new();
    o.insert("name".into(), json!(f.name));
    o.insert("path".into(), json!(f.path));
    o.insert("type".into(), json!(f.xml_type));
    o.insert("short_type".into(), json!(f.short_type));
    if let Some(v) = &f.value {
        o.extend(v.json.clone());
    }
    for (k, v) in [("units", &f.units), ("range", &f.range), ("help", &f.help), ("markers", &f.markers)] {
        if let Some(v) = v {
            o.insert(k.into(), json!(v));
        }
    }
    if let Some(off) = f.offset {
        o.insert("offset".into(), json!(off));
    }
    if let Some((_, list)) = &f.options {
        o.insert("options".into(), json!(list));
    }
    if let Some(s) = &f.state {
        if s.error {
            o.insert("error".into(), json!(s.key));
            o.insert("reason".into(), json!(s.text));
        } else {
            o.insert("state".into(), json!(s.key));
        }
    }
    if let Some(w) = &f.warning {
        o.insert("warning".into(), json!(w));
    }
    if f.hidden {
        o.insert("hidden".into(), json!(true));
    }
    if let Some(c) = &f.container {
        if let Some(n) = c.count {
            o.insert("count".into(), json!(n));
        }
        if !c.labels.is_empty() {
            o.insert("labels".into(), json!(c.labels));
        }
        if let Some(i) = c.index {
            o.insert("index".into(), json!(i));
        }
        if c.stub {
            o.insert("expand".into(), json!(f.path));
        }
        if !c.children.is_empty() {
            o.insert("children".into(), nodes_json(&c.children));
        }
        if !c.not_read.is_empty() {
            o.insert("not_read".into(), json!(c.not_read));
        }
    }
    Value::Object(o)
}

fn nodes_json(nodes: &[ViewNode]) -> Value {
    Value::Array(
        nodes
            .iter()
            .map(|n| match n {
                ViewNode::Heading { kind, title, text } => {
                    let mut o = Map::new();
                    o.insert("heading".into(), json!(title));
                    o.insert("kind".into(), json!(kind.key()));
                    if let Some(t) = text {
                        o.insert("text".into(), json!(t));
                    }
                    Value::Object(o)
                }
                ViewNode::Field(f) => field_json(f),
            })
            .collect(),
    )
}

impl TagView {
    /// `text` (`aligned = false`, what the AI reads) or `table` (people).
    pub fn render_lines(&self, aligned: bool) -> String {
        let mut out = format!("{}\n", self.title);
        for l in &self.title_lines {
            out.push_str(&format!("  {l}\n"));
        }
        if self.brief {
            write_brief(&self.nodes, &mut out);
        } else {
            let mut lines = Lines { out, aligned, run: Vec::new() };
            write_nodes(&self.nodes, 0, &mut lines);
            out = lines.out;
        }
        if !self.legend.is_empty() {
            out.push_str("legend:\n");
            for l in &self.legend {
                out.push_str(&format!("  {l}\n"));
            }
        }
        for h in &self.hints {
            out.push_str(&format!("hint: {h}\n"));
        }
        out
    }

    pub fn render_json(&self) -> Value {
        let mut o = self.title_json.clone();
        o.insert("fields".into(), nodes_json(&self.nodes));
        if !self.hints.is_empty() {
            o.insert("hints".into(), json!(self.hints));
        }
        Value::Object(o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str, ty: &str, value: &str) -> ViewNode {
        ViewNode::Field(Box::new(FieldView {
            name: name.into(),
            path: name.into(),
            xml_type: "_field_real",
            short_type: ty.into(),
            value: Some(Shown { text: value.into(), json: Map::from_iter([("value".into(), json!(value))]) }),
            ..FieldView::default()
        }))
    }

    fn sample() -> TagView {
        let mut barrels = FieldView {
            name: "barrels".into(),
            path: "barrels".into(),
            xml_type: "_field_block_64",
            short_type: "block[2]".into(),
            container: Some(Container {
                summary: Some("{primary, secondary}  showing [0] primary".into()),
                count: Some(2),
                labels: vec!["primary".into(), "secondary".into()],
                index: Some(0),
                children: vec![field("rounds per second", "bounds", "8.5..8.5"), field("x", "real", "1")],
                ..Container::default()
            }),
            ..FieldView::default()
        };
        if let Some(ViewNode::Field(f)) = barrels.container.as_mut().unwrap().children.first_mut() {
            f.path = "barrels[0].rounds per second".into();
        }
        TagView {
            title: "# weap test.weapon  0x00000010  build=forge".into(),
            nodes: vec![
                ViewNode::Heading { kind: HeadingKind::ClassSection, title: "WEAPON".into(), text: None },
                field("bounding radius", "real", "0.17"),
                field("mass", "real", "2"),
                ViewNode::Heading {
                    kind: HeadingKind::Explanation,
                    title: "Firing".into(),
                    text: Some("How it fires. […]".into()),
                },
                ViewNode::Field(Box::new(barrels)),
            ],
            ..TagView::default()
        }
    }

    #[test]
    fn text_separates_columns_with_two_spaces() {
        assert_eq!(
            sample().render_lines(false),
            "# weap test.weapon  0x00000010  build=forge\n\
             ## WEAPON\n\
             bounding radius  real  0.17\n\
             mass  real  2\n\
             ### Firing\n\
             > How it fires. […]\n\
             ### barrels  block[2]  {primary, secondary}  showing [0] primary\n\
             \x20 rounds per second  bounds  8.5..8.5\n\
             \x20 x  real  1\n"
        );
    }

    #[test]
    fn table_aligns_columns_within_each_run() {
        let t = sample().render_lines(true);
        assert!(t.contains("bounding radius  real  0.17\nmass             real  2\n"), "{t}");
        assert!(t.contains("  rounds per second  bounds  8.5..8.5\n  x                  real    1\n"), "{t}");
    }

    #[test]
    fn json_is_the_same_tree() {
        let v = sample().render_json();
        let fields = v["fields"].as_array().unwrap();
        assert_eq!(fields[0], json!({ "heading": "WEAPON", "kind": "class_section" }));
        assert_eq!(fields[1]["value"], "0.17");
        assert_eq!(fields[4]["count"], 2);
        assert_eq!(fields[4]["children"][0]["path"], "barrels[0].rounds per second");
    }

    #[test]
    fn brief_is_paths_and_values() {
        let mut v = sample();
        v.brief = true;
        let t = v.render_lines(false);
        assert!(t.contains("\nbounding radius  0.17\n"), "{t}");
        assert!(t.contains("\nbarrels[0].rounds per second  8.5..8.5\n"), "{t}");
        assert!(!t.contains("##"), "{t}");
    }
}
