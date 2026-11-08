//! Definitions without tag data, as `get_definition` and `outline` show them
//! (PRD §6.5): the same view and the same rules as `read_tag` (headings,
//! hidden fields, options, depth), from the definition alone.

use h5_defs::definition::{Field, FieldKind};
use h5_defs::field_type::FieldType;
use h5_meta::{Body, Content, FieldPath, PathError, Section, parse_path};

use crate::output::marker;
use crate::tag_view::{
    Detail, Options, Role, ViewSettings, entry_labels, is_hidden, join, label_list, role, short_type, shown_fields,
};
use crate::view::{Container, FieldView, HeadingKind, PATH_TARGET, Shown, ViewNode};

const AUTO_ENUM_OPTIONS: usize = 12;

fn is_container(f: &Field) -> bool {
    matches!(f.kind, FieldKind::Known(FieldType::Struct | FieldType::Array | FieldType::Block64))
        && !f.is_mapping_function()
}

fn fields_text(n: usize) -> String {
    if n == 1 { "(1 field)".into() } else { format!("({n} fields)") }
}

struct DefView<'d> {
    /// The definition's top-level fields (block index targets are looked up there).
    root: &'d [Field],
    settings: ViewSettings,
}

impl DefView<'_> {
    fn body(&self, fields: &[Field], path: &str, level: u32, top: bool) -> Vec<ViewNode> {
        let mut out = Vec::new();
        let mut offset = 0u32;
        for (i, f) in fields.iter().enumerate() {
            let at = offset;
            offset += f.size().unwrap_or(0);
            match role(fields, i, top, self.settings.detail) {
                Role::Skip => continue,
                Role::Heading(h) => {
                    out.push(h);
                    continue;
                }
                Role::Field => {}
            }
            if is_hidden(f) && !self.settings.show_hidden {
                continue;
            }
            let mut v = self.field(f, &join(path, f.step.as_deref().unwrap_or_default()), level);
            v.hidden = is_hidden(f);
            if self.settings.detail == Detail::Full {
                v.offset = Some(at);
                v.help = f.name.help.clone();
                v.markers = (!f.name.markers.is_empty()).then(|| f.name.markers.clone());
            }
            out.push(ViewNode::Field(Box::new(v)));
        }
        out
    }

    fn field(&self, f: &Field, path: &str, level: u32) -> FieldView {
        let FieldKind::Known(t) = f.kind else {
            let FieldKind::Unknown(name) = &f.kind else { unreachable!() };
            return FieldView {
                name: f.step.clone().unwrap_or_default(),
                path: path.to_owned(),
                short_type: name.clone(),
                ..FieldView::default()
            };
        };
        let mut v = FieldView {
            name: f.step.clone().unwrap_or_default(),
            path: path.to_owned(),
            xml_type: t.xml_name(),
            short_type: if t == FieldType::Block64 { "block".into() } else { short_type(t, None) },
            ..FieldView::default()
        };
        if self.settings.detail != Detail::Brief {
            v.units = f.name.units.clone();
            v.range = f.name.range.clone();
        }
        if f.is_mapping_function() {
            v.short_type = "function".into();
            return v;
        }
        if is_container(f) {
            let mut c = Container::default();
            if let Some(max) = f.attr("max") {
                c.summary = Some(format!("(max {max})"));
            }
            if level < self.settings.depth {
                c.children = self.body(&f.children, path, level + 1, false);
            } else {
                c.stub = true;
            }
            v.container = Some(c);
            return v;
        }
        let shown = |text: String| Some(Shown { text, json: serde_json::Map::new() });
        match t {
            FieldType::TagReference64 => {
                let valid: Vec<&str> = f
                    .attr("validtags")
                    .map(|s| s.split([',', ' ']).filter(|c| !c.is_empty()).collect())
                    .unwrap_or_default();
                v.value = shown(format!("[{}]", valid.join(", ")));
            }
            FieldType::CharBlockIndex
            | FieldType::ShortBlockIndex
            | FieldType::LongBlockIndex
            | FieldType::CustomShortBlockIndex
            | FieldType::CustomLongBlockIndex => {
                if let Some(target) = self.target_block(f) {
                    v.value = shown(format!("→ {target}"));
                }
            }
            _ if !f.options.is_empty() => v.options = self.options(f, t),
            _ => {}
        }
        v
    }

    /// Every option, per `options=` (PRD §7.5); hidden ones only with `show_hidden`.
    fn options(&self, f: &Field, t: FieldType) -> Option<(String, Vec<String>)> {
        let names: Vec<String> = f
            .options
            .iter()
            .filter(|o| self.settings.show_hidden || !o.is_hidden())
            .map(|o| o.display.clone())
            .collect();
        let flags = short_type(t, None).starts_with("flags");
        let list = || format!("{{{}}}", names.join(if flags { ", " } else { "|" }));
        match self.settings.options {
            Options::None => None,
            _ if names.is_empty() => None,
            Options::All => Some((list(), names)),
            Options::Auto if names.len() <= AUTO_ENUM_OPTIONS => Some((list(), names)),
            Options::Auto => Some((format!("({} options)", names.len()), names)),
        }
    }

    /// The tag block a block index points into: `targetGuid` → a block's
    /// `srcguid`, else `targetName` → its `srcName` (PRD §5.3).
    fn target_block(&self, index: &Field) -> Option<String> {
        fn find<'f>(fields: &'f [Field], wanted: &dyn Fn(&Field) -> bool) -> Option<&'f Field> {
            fields.iter().find_map(|f| {
                if f.kind == FieldKind::Known(FieldType::Block64) && wanted(f) {
                    return Some(f);
                }
                find(&f.children, wanted)
            })
        }
        let wanted = |f: &Field| match (index.attr("targetGuid"), index.attr("targetName")) {
            (Some(g), _) => f.attr("srcguid").is_some_and(|s| s.eq_ignore_ascii_case(g)),
            (None, Some(n)) => f.attr("srcName") == Some(n),
            (None, None) => false,
        };
        find(self.root, &wanted).and_then(|f| f.step.clone()).or_else(|| index.attr("targetName").map(str::to_owned))
    }
}

/// `get_definition`: the fields of a definition, or of the field at `path`
/// (tag blocks are stepped into without an index; `[i]`/`[*]` are ignored).
pub fn definition_view(fields: &[Field], path: &str, settings: ViewSettings) -> Result<Vec<ViewNode>, PathError> {
    let view = DefView { root: fields, settings };
    let p = parse_path(fields, path)?;
    let Some(last) = p.steps.last() else { return Ok(view.body(fields, "", 0, true)) };
    let plain = FieldPath { steps: p.steps.iter().map(|s| h5_meta::Step { field: s.field, select: None }).collect() };
    let shown = plain.display(fields);
    let mut level = fields;
    for s in &p.steps[..p.steps.len() - 1] {
        level = &level[s.field].children;
    }
    let f = &level[last.field];
    let mut out =
        vec![ViewNode::Heading { kind: HeadingKind::Target, title: format!("{shown}{PATH_TARGET}"), text: None }];
    if is_container(f) {
        out.extend(view.body(&f.children, &shown, 0, false));
    } else {
        // Like read_tag's path= reads: a single value is named by its full path.
        let mut v = view.field(f, &shown, 0);
        v.name = shown.clone();
        out.push(ViewNode::Field(Box::new(v)));
    }
    Ok(out)
}

/// `get_definition(section=)`: the section's fields (PRD §6.4.3).
pub fn definition_section(fields: &[Field], sec: &Section, settings: ViewSettings) -> Vec<ViewNode> {
    let view = DefView { root: fields, settings };
    let mut level = fields;
    for s in &sec.parent.steps {
        level = &level[s.field].children;
    }
    let end = sec.end.min(level.len());
    let start = (sec.start + 1).min(end);
    let path = sec.parent.display(fields);
    let mut out =
        vec![ViewNode::Heading { kind: HeadingKind::Target, title: format!("{}  (section)", sec.heading), text: None }];
    out.extend(view.body(&level[start..end], &path, 0, sec.parent.steps.is_empty()));
    out
}

/// One outline node: a container with its direct field count and path.
fn outline_node(f: &Field, path: &str, short: String, extra: Option<String>, children: Vec<ViewNode>) -> ViewNode {
    let direct = shown_fields(&f.children, false).count();
    let mut summary = extra.map(|e| format!("{e}  ")).unwrap_or_default();
    summary.push_str(&format!("{}  path={path}", fields_text(direct)));
    ViewNode::Field(Box::new(FieldView {
        name: f.step.clone().unwrap_or_default(),
        path: path.to_owned(),
        xml_type: match f.kind {
            FieldKind::Known(t) => t.xml_name(),
            FieldKind::Unknown(_) => "",
        },
        short_type: short,
        container: Some(Container { summary: Some(summary), children, ..Container::default() }),
        ..FieldView::default()
    }))
}

/// Headings of an outline: their titles only.
fn outline_heading(fields: &[Field], i: usize, top: bool) -> Option<ViewNode> {
    match role(fields, i, top, Detail::Normal) {
        Role::Heading(ViewNode::Heading { kind, title, .. }) => Some(ViewNode::Heading { kind, title, text: None }),
        _ => None,
    }
}

/// `outline(class=)`: the section tree of a definition, `depth` levels deep.
pub fn outline_definition(fields: &[Field], depth: u32) -> Vec<ViewNode> {
    Outliner { depth, of_tag: false }.visit(fields, None, "", 0)
}

fn short_type_of(f: &Field) -> String {
    match f.kind {
        FieldKind::Known(t) => short_type(t, None),
        FieldKind::Unknown(_) => String::new(),
    }
}

/// `outline(tag=)`: the section tree of a tag, from its definition (D70): entry
/// counts and labels where the tag was read (nested blocks as entry 0 holds
/// them), `(not read)` where it wasn't.
pub fn outline_tag(body: &Body<'_>, fields: &[Field], depth: u32) -> Vec<ViewNode> {
    Outliner { depth, of_tag: true }.visit(fields, Some(body), "", 0)
}

/// One walk for both outlines: the definition's containers, with what a read
/// found in them.
struct Outliner {
    depth: u32,
    /// A tag's outline: containers with nothing read say `(not read)`.
    of_tag: bool,
}

impl Outliner {
    /// `read`: what was read of `fields` (its children line up with them, up
    /// to where the read stopped); `None` when nothing was, or for a class.
    fn visit(&self, fields: &[Field], read: Option<&Body<'_>>, path: &str, level: u32) -> Vec<ViewNode> {
        let mut out = Vec::new();
        for (i, f) in fields.iter().enumerate() {
            if let Some(h) = outline_heading(fields, i, level == 0) {
                out.push(h);
                continue;
            }
            if !is_container(f) || f.is_internal() || is_hidden(f) {
                continue;
            }
            let p = join(path, f.step.as_deref().unwrap_or_default());
            let deeper = level + 1 < self.depth;
            let below = |read: Option<&Body<'_>>, at: &str| {
                if deeper { self.visit(&f.children, read, at, level + 1) } else { Vec::new() }
            };
            let node = match read.and_then(|b| b.children.get(i)).map(|n| &n.content) {
                Some(Content::Block(b)) => {
                    let extra = label_list(&entry_labels(&b.entries));
                    let children = match b.entries.first() {
                        Some(e) => below(Some(e), &format!("{p}[0]")),
                        None => Vec::new(),
                    };
                    outline_node(f, &p, format!("block[{}]", b.count), extra, children)
                }
                Some(Content::Struct(b) | Content::Array(b)) => {
                    outline_node(f, &p, short_type_of(f), None, below(Some(b), &p))
                }
                _ => {
                    let short =
                        if f.kind == FieldKind::Known(FieldType::Block64) { "block".into() } else { short_type_of(f) };
                    let not_read = self.of_tag.then(|| marker::NOT_READ.to_owned());
                    outline_node(f, &p, short, not_read, below(None, &p))
                }
            };
            out.push(node);
        }
        out
    }
}

#[cfg(test)]
#[path = "def_view_tests.rs"]
mod tests;
