//! A tag or definition as the AI and people read it (PRD §7.2–§7.7): one tree
//! of nodes, rendered as `text`, `table` or JSON. Every rendering comes from
//! the same nodes (PRD §13). `text` separates columns with two spaces;
//! `table` aligns them (decisions D36).

use std::borrow::Cow;

use serde_json::{Map, Value, json};

/// Ends the title of the heading that opens a `path=` read: `barrels[0]  (path)`.
pub const PATH_TARGET: &str = "  (path)";

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
/// The value column: the value, then its state (`(none)`, `!! …`).
fn value_column(f: &FieldView) -> String {
    let value = f.value.as_ref().map(|v| v.text.clone()).unwrap_or_default();
    match f.state.as_ref().filter(|s| !s.text.is_empty()) {
        Some(s) if value.is_empty() => s.text.clone(),
        Some(s) => format!("{value} {}", s.text),
        None => value,
    }
}

fn columns(f: &FieldView) -> Vec<String> {
    let mut cols = vec![f.name.clone(), f.short_type.clone()];
    cols.push(value_column(f));
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

/// One `detail=brief` row: the block entry (or read target) it lies in, its
/// full path and its value.
struct BriefRow<'v> {
    /// Index into the groups' paths.
    group: Option<usize>,
    path: &'v str,
    value: String,
}

/// The rows of `nodes`, and the paths of the groups they lie in (one per
/// read target or block entry, not one copy per row).
#[derive(Default)]
struct Brief<'v> {
    groups: Vec<Cow<'v, str>>,
    rows: Vec<BriefRow<'v>>,
}

impl<'v> Brief<'v> {
    fn collect(&mut self, nodes: &'v [ViewNode], mut group: Option<usize>) {
        for n in nodes {
            match n {
                // A read's target, or one entry of a `[*]` read.
                ViewNode::Heading { kind: HeadingKind::Target, title, .. } => {
                    self.groups.push(Cow::Borrowed(title.strip_suffix(PATH_TARGET).unwrap_or(title)));
                    group = Some(self.groups.len() - 1);
                }
                ViewNode::Heading { .. } => {}
                ViewNode::Field(f) => match &f.container {
                    Some(c) if !c.children.is_empty() => {
                        // What its heading would say that its rows can't: a
                        // block's entry count, a warning, an error (D45).
                        if c.count.is_some() || f.warning.is_some() || f.state.is_some() {
                            self.rows.push(BriefRow { group, path: &f.path, value: brief_value(f) });
                        }
                        let entry = c.index.map(|i| {
                            self.groups.push(Cow::Owned(format!("{}[{i}]", f.path)));
                            self.groups.len() - 1
                        });
                        self.collect(&c.children, entry.or(group));
                    }
                    _ => self.rows.push(BriefRow { group, path: &f.path, value: brief_value(f) }),
                },
            }
        }
    }
}

/// A brief row's value: the value column (for a container, its type), plus
/// what its heading says: the entry shown, not expanded (`→ path=`), a warning.
fn brief_value(f: &FieldView) -> String {
    use std::fmt::Write;
    let mut s = value_column(f);
    if s.is_empty() {
        s = f.short_type.clone();
    }
    // Writing to a String can't fail.
    if let Some(c) = &f.container {
        if let Some(i) = c.index.filter(|_| !c.children.is_empty()) {
            let _ = write!(s, "  showing [{i}]");
            // Brief has no labels; the count and where the rest are (D48).
            if let Some(n) = c.count.filter(|&n| n > 1) {
                let _ = write!(s, " of {n}  → path={}[*]", f.path);
            }
        }
        if c.stub {
            let _ = write!(s, "  → path={}", f.path);
        }
    }
    if let Some(w) = &f.warning {
        let _ = write!(s, "  warning: {w}");
    }
    s
}

/// `<full path>  <value>` lines; two or more rows in a row from one block
/// entry go under an `in <entry path>:` line, indented, with paths relative
/// to it (decisions D40).
fn write_brief(nodes: &[ViewNode], out: &mut String) {
    use std::fmt::Write;
    let mut b = Brief::default();
    b.collect(nodes, None);
    let group = |r: &BriefRow<'_>| r.group.map(|g| &*b.groups[g]);
    // A row's path inside its group; `None` if the path doesn't start with it
    // (a section heading, a single value read).
    let inside: Vec<Option<&str>> =
        b.rows.iter().map(|r| group(r).and_then(|g| r.path.strip_prefix(g)?.strip_prefix('.'))).collect();
    let rows = &b.rows;
    let mut i = 0;
    // Writing to a String can't fail.
    while i < rows.len() {
        let run = (i..rows.len()).take_while(|&k| inside[k].is_some() && rows[k].group == rows[i].group).count();
        if run < 2 {
            let _ = writeln!(out, "{}  {}", rows[i].path, rows[i].value);
            i += 1;
            continue;
        }
        let _ = writeln!(out, "in {}:", group(&rows[i]).unwrap_or_default());
        for k in i..i + run {
            let _ = writeln!(out, "  {}  {}", inside[k].unwrap_or_default(), rows[k].value);
        }
        i += run;
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

    fn brief_lines(nodes: Vec<ViewNode>) -> String {
        TagView { title: "# t".into(), nodes, brief: true, ..TagView::default() }.render_lines(false)
    }

    fn at(path: &str, value: &str) -> ViewNode {
        let ViewNode::Field(mut f) = field(path, "real", value) else { unreachable!() };
        f.path = path.into();
        ViewNode::Field(f)
    }

    /// Decisions D40: the rows of one block entry share an `in <path>:` line.
    #[test]
    fn brief_is_paths_and_values_grouped_by_block_entry() {
        let mut v = sample();
        v.brief = true;
        if let Some(ViewNode::Field(f)) = v.nodes.last_mut() {
            let ViewNode::Field(x) = &mut f.container.as_mut().unwrap().children[1] else { unreachable!() };
            x.path = "barrels[0].x".into();
        }
        let t = v.render_lines(false);
        assert!(t.contains("\nbounding radius  0.17\n"), "{t}");
        assert!(t.contains("\nin barrels[0]:\n  rounds per second  8.5..8.5\n  x  1\n"), "{t}");
        assert!(!t.contains("##"), "{t}");
        // The block's own row keeps its entry count (D45).
        assert!(t.contains("\nbarrels  block[2]  showing [0] of 2  → path=barrels[*]\nin barrels[0]:\n"), "{t}");
    }

    /// D45: what a heading carries survives brief: a block's count, a
    /// warning, and where a collapsed struct can be read.
    #[test]
    fn brief_keeps_counts_warnings_and_stubs() {
        let container = |name: &str, short: &str, c: Container, warning: Option<&str>| {
            ViewNode::Field(Box::new(FieldView {
                name: name.into(),
                path: name.into(),
                short_type: short.into(),
                container: Some(c),
                warning: warning.map(str::to_owned),
                ..FieldView::default()
            }))
        };
        let inner = vec![at("penalties.a", "1"), at("penalties.b", "2")];
        let t = brief_lines(vec![
            container(
                "penalties",
                "struct",
                Container { children: inner, ..Container::default() },
                Some("definition GUID mismatch"),
            ),
            container(
                "plain",
                "struct",
                Container { children: vec![at("plain.c", "3")], ..Container::default() },
                None,
            ),
            container("kick", "struct", Container { stub: true, ..Container::default() }, None),
        ]);
        assert!(t.contains("\npenalties  struct  warning: definition GUID mismatch\n"), "{t}");
        assert!(!t.contains("\nplain  struct"), "an expanded struct without news has no row: {t}");
        assert!(t.contains("\nkick  struct  → path=kick\n"), "{t}");
    }

    #[test]
    fn a_brief_group_of_one_row_keeps_the_full_path() {
        let mut v = sample();
        v.brief = true;
        if let Some(ViewNode::Field(f)) = v.nodes.last_mut() {
            f.container.as_mut().unwrap().children.truncate(1);
        }
        let t = v.render_lines(false);
        assert!(t.contains("\nbarrels[0].rounds per second  8.5..8.5\n"), "{t}");
        assert!(!t.contains("in barrels"), "{t}");
    }

    /// A `[*]` read: each entry's target heading starts its group; a `path=`
    /// read of one entry: the `(path)` heading does.
    #[test]
    fn brief_groups_the_entries_of_a_read() {
        let target = |title: &str| ViewNode::Heading { kind: HeadingKind::Target, title: title.into(), text: None };
        let t = brief_lines(vec![
            target("list[*]  (path)"),
            target("list[0]"),
            at("list[0].a", "1"),
            at("list[0].b", "2"),
            target("list[1]"),
            at("list[1].a", "3"),
            at("list[1].b", "4"),
        ]);
        assert!(t.ends_with("# t\nin list[0]:\n  a  1\n  b  2\nin list[1]:\n  a  3\n  b  4\n"), "{t}");
        let t = brief_lines(vec![target("list[1]  (path)"), at("list[1].a", "3"), at("list[1].b", "4")]);
        assert!(t.ends_with("# t\nin list[1]:\n  a  3\n  b  4\n"), "{t}");
        // A single value read keeps its full path.
        let t = brief_lines(vec![target("list[1].a  (path)"), at("list[1].a", "3")]);
        assert!(t.ends_with("# t\nlist[1].a  3\n"), "{t}");
    }
}
