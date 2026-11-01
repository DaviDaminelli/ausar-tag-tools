//! A tag read with its definition (h5-meta), as the view the AI reads
//! (PRD §6.4.4 value rules, §7, §8 states; AGENT_GUIDE §4). Presentation: it
//! formats values, it doesn't read data. What only the build knows (tag
//! names, class names, resources) comes through [`Lookup`].

use h5_defs::definition::{Custom, Field, FieldKind, custom_role};
use h5_defs::field_type::FieldType;
use h5_defs::name::Name;
use h5_meta::{Block, Body, Content, EnumOption, Problem, Selected, Value, target_block};
use h5_module::{FourCc, GlobalId};
use serde_json::{Map, json};

use crate::output::{format_size, marker};
use crate::view::{Container, FieldView, HeadingKind, Shown, State, ViewNode};

/// How much to show (PRD §7.6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Detail {
    /// Field paths and values only.
    Brief,
    /// + headings, first line of explanations, units, ranges, options.
    #[default]
    Normal,
    /// + help text, full explanations, offsets, markers, `(not read)` lines.
    Full,
}

/// Which enum and flag options to list (PRD §7.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Options {
    /// Enums with ≤ 12 visible options list them all; else the label + `(+N options)`.
    #[default]
    Auto,
    All,
    None,
}

#[derive(Clone, Copy, Debug)]
pub struct ViewSettings {
    /// Nested levels expanded below the requested node (PRD §6.4.1).
    pub depth: u32,
    pub detail: Detail,
    pub options: Options,
    pub show_hidden: bool,
}

/// A tag resource as the index describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceInfo {
    pub kind: String,
    pub size: u64,
    pub stored: bool,
}

/// What the build knows that the tag doesn't.
pub trait Lookup {
    /// The display name of an indexed tag.
    fn tag_name(&self, id: GlobalId) -> Option<String>;
    fn class_name(&self, code: FourCc) -> String;
    /// The tag's resource `k` (PRD §12 Q9: resource field k ↔ resource k).
    fn resource(&self, k: u32) -> Option<ResourceInfo>;
}

/// The build behind a view: tag and class names, and the resources of one copy.
pub struct BuildLookup<'b> {
    pub build: &'b crate::catalog::Build,
    pub copy: crate::catalog::Copy,
}

impl Lookup for BuildLookup<'_> {
    fn tag_name(&self, id: GlobalId) -> Option<String> {
        self.build.tag_by_id(id).map(|t| self.build.display_name(t))
    }

    fn class_name(&self, code: FourCc) -> String {
        self.build.class_name(code).into_owned()
    }

    fn resource(&self, k: u32) -> Option<ResourceInfo> {
        let file = *self.build.entry(self.copy).resources.get(k as usize)?;
        let r = &self.build.module(self.copy).entries[file as usize];
        Some(ResourceInfo { kind: r.resource_name()?.kind.to_owned(), size: r.uncompressed_size, stored: r.stored })
    }
}

const AUTO_ENUM_OPTIONS: usize = 12;
const MAX_LABELS: usize = 8;

fn hidden_type(f: &Field) -> bool {
    matches!(f.kind, FieldKind::Known(FieldType::Pad | FieldType::Skip | FieldType::Tag))
}

/// Hidden by default (PRD §7.6): padding, `_field_tag`, and `!` fields.
pub(crate) fn is_hidden(f: &Field) -> bool {
    hidden_type(f) || f.name.is_hidden()
}

pub(crate) fn short_type(t: FieldType, count: Option<u32>) -> String {
    use FieldType as T;
    let s = match t {
        T::Real | T::RealFraction | T::Angle => "real",
        T::CharInteger | T::ByteInteger => "int8",
        T::ShortInteger | T::WordInteger => "int16",
        T::LongInteger | T::DwordInteger => "int32",
        T::Int64Integer | T::QwordInteger => "int64",
        T::CharEnum => "enum8",
        T::ShortEnum => "enum16",
        T::LongEnum => "enum32",
        T::ByteFlags | T::ByteBlockFlags => "flags8",
        T::WordFlags | T::WordBlockFlags => "flags16",
        T::LongFlags | T::LongBlockFlags => "flags32",
        T::RealBounds | T::AngleBounds | T::FractionBounds | T::ShortIntegerBounds => "bounds",
        T::RealPoint2d | T::Point2d => "point2",
        T::RealPoint3d => "point3",
        T::RealVector2d => "vec2",
        T::RealVector3d => "vec3",
        T::RealEulerAngles2d => "euler2",
        T::RealEulerAngles3d => "euler3",
        T::RealQuaternion => "quat",
        T::RealPlane2d => "plane2",
        T::RealPlane3d => "plane3",
        T::Rectangle2d => "rect2",
        T::RgbColor | T::RealRgbColor => "rgb",
        T::ArgbColor | T::RealArgbColor => "argb",
        T::RealHsvColor => "hsv",
        T::RealAhsvColor => "ahsv",
        T::String | T::LongString => "string",
        T::StringId | T::OldStringId => "stringid",
        T::TagReference64 => "tagref",
        T::CharBlockIndex | T::ShortBlockIndex | T::LongBlockIndex => "blkidx",
        T::CustomShortBlockIndex | T::CustomLongBlockIndex => "blkidx",
        T::Struct => "struct",
        T::Array => "array",
        T::Data64 => "data",
        T::PageableResource64 => "resource",
        T::Pad | T::Skip => "pad",
        T::Tag => "tag",
        T::Explanation | T::Custom => "",
        T::Block64 => return format!("block[{}]", count.unwrap_or(0)),
    };
    s.to_owned()
}

fn state(text: impl Into<String>, key: &'static str) -> Option<State> {
    Some(State { text: text.into(), key, error: false })
}

fn error(text: impl Into<String>) -> Option<State> {
    Some(State { text: text.into(), key: "error", error: true })
}

fn plain(text: String, json: serde_json::Value) -> Shown {
    Shown { text, json: Map::from_iter([("value".into(), json)]) }
}

/// The tag-local name of a reference's target: the tag file name without folders.
fn file_name(display: &str) -> &str {
    display.rsplit('\\').next().unwrap_or(display)
}

/// Why a struct or block isn't (fully) shown (PRD §8).
pub fn problem_text(p: &Problem, path: &str) -> String {
    match p {
        Problem::SizeMismatch { definition, tag } => {
            format!("!! definition size mismatch at {path} (definition {definition} bytes, tag {tag} bytes)")
        }
        Problem::UnknownType { at, type_name } => format!("!! read stopped at {at}: unknown field type {type_name}"),
        Problem::UnknownSize { at } => format!("!! read stopped at {at}: the field's size is unknown"),
        Problem::NoDataBlock => format!("!! read failed at {path}: the tag header lists no data block for it"),
        Problem::OutOfBounds { at } => format!("!! read failed at {at}: the data lies outside its data block"),
    }
}

/// What field `fields[i]` of a definition is in a view (tags and definitions alike).
pub(crate) enum Role {
    /// Not shown: the internal header fields, editor widgets, and headings at `detail=brief`.
    Skip,
    Heading(ViewNode),
    Field,
}

/// The role of `fields[i]`; `top`: the fields are the main struct's.
pub(crate) fn role(fields: &[Field], i: usize, top: bool, detail: Detail) -> Role {
    let f = &fields[i];
    if f.is_internal() {
        return Role::Skip;
    }
    match f.kind {
        FieldKind::Known(FieldType::Explanation) => {
            // An explanation with no text naming the next field only repeats
            // that field's own heading (decisions D36).
            let names_next = f.text.as_deref().is_none_or(|t| t.trim().is_empty())
                && fields.get(i + 1).is_some_and(|n| n.name.display.eq_ignore_ascii_case(&f.name.display));
            if names_next || detail == Detail::Brief {
                return Role::Skip;
            }
            let text = f.text.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(|t| match detail {
                Detail::Full => t.to_owned(),
                _ => {
                    let mut lines = t.lines();
                    let first = lines.next().unwrap_or_default().trim_end().to_owned();
                    if lines.next().is_some() { format!("{first} […]") } else { first }
                }
            });
            Role::Heading(ViewNode::Heading { kind: HeadingKind::Explanation, title: f.name.display.clone(), text })
        }
        FieldKind::Known(FieldType::Custom) => {
            let (kind, title) = match custom_role(fields, i) {
                Some(Custom::ClassSection(t)) if top => (HeadingKind::ClassSection, t),
                Some(Custom::ClassSection(t) | Custom::Group(t)) => (HeadingKind::Group, t),
                _ => return Role::Skip,
            };
            if detail == Detail::Brief {
                Role::Skip
            } else {
                Role::Heading(ViewNode::Heading { kind, title, text: None })
            }
        }
        _ => Role::Field,
    }
}

struct Converter<'s, 'l> {
    settings: ViewSettings,
    lookup: &'l dyn Lookup,
    /// Enclosing structs (h5-meta bodies) and their paths, for block indices.
    scopes: Vec<(&'s Body<'s>, String)>,
}

/// Entry labels as a block heading shows them: at most 8, then `+N more` (PRD §7.3).
pub(crate) fn label_list(labels: &[String]) -> String {
    let shown: Vec<&str> = labels.iter().take(MAX_LABELS).map(String::as_str).collect();
    let more = labels.len().saturating_sub(MAX_LABELS);
    let more = if more > 0 { format!(", +{more} more") } else { String::new() };
    format!("{{{}{more}}}", shown.join(", "))
}

pub(crate) fn join(path: &str, step: &str) -> String {
    if path.is_empty() { step.to_owned() } else { format!("{path}.{step}") }
}

impl<'s> Converter<'s, '_> {
    /// The visible options of `field` (all with `show_hidden`), with their positions.
    fn visible<'f>(&self, options: &'f [Name]) -> Vec<(usize, &'f Name)> {
        options.iter().enumerate().filter(|(_, o)| self.settings.show_hidden || !o.is_hidden()).collect()
    }

    fn body(&mut self, body: &'s Body<'s>, fields: &'s [Field], path: &str, level: u32) -> Container {
        self.scopes.push((body, path.to_owned()));
        let mut c = Container::default();
        let mut hidden = 0usize;
        for (i, n) in body.children.iter().enumerate() {
            let f = n.field;
            match role(fields, i, level == 0 && path.is_empty(), self.settings.detail) {
                Role::Skip => continue,
                Role::Heading(h) => {
                    c.children.push(h);
                    continue;
                }
                Role::Field => {}
            }
            if is_hidden(f) && !self.settings.show_hidden {
                hidden += 1;
                continue;
            }
            let step = n.name.unwrap_or_default();
            let p = join(path, step);
            let mut v = self.field(n, &p, level);
            v.hidden = is_hidden(f);
            if self.settings.detail == Detail::Full {
                v.offset = Some(n.offset);
                v.help = f.name.help.clone();
                v.markers = (!f.name.markers.is_empty()).then(|| f.name.markers.clone());
            }
            c.children.push(ViewNode::Field(Box::new(v)));
        }
        if hidden > 0 && !c.children.iter().any(|n| matches!(n, ViewNode::Field(_))) {
            c.summary = Some(format!("({hidden} hidden fields)"));
        }
        if self.settings.detail == Detail::Full && body.problem.is_some() {
            c.not_read = fields.iter().skip(body.children.len() + 1).filter_map(|f| f.step.clone()).collect();
        }
        self.scopes.pop();
        c
    }

    fn field(&mut self, n: &'s h5_meta::Node<'s>, path: &str, level: u32) -> FieldView {
        let f = n.field;
        let t = match f.kind {
            FieldKind::Known(t) => t,
            FieldKind::Unknown(_) => unreachable!("the reader stops before unknown types"),
        };
        let mut v = FieldView {
            name: n.name.unwrap_or_default().to_owned(),
            path: path.to_owned(),
            xml_type: t.xml_name(),
            short_type: short_type(t, None),
            ..FieldView::default()
        };
        if self.settings.detail != Detail::Brief {
            v.units = f.name.units.clone();
            v.range = f.name.range.clone();
        }
        match &n.content {
            Content::Value(value) => self.value(n, value, &mut v),
            Content::Struct(b) | Content::Array(b) if f.is_mapping_function() => {
                // One line (PRD §7.2): the curve's data size.
                let size = b.children.iter().find_map(|c| match &c.content {
                    Content::Value(Value::Data(d)) => Some(d.size),
                    _ => None,
                });
                v.short_type = "function".into();
                v.value = Some(plain(format!("(curve, {} bytes)", size.unwrap_or(0)), json!(size.unwrap_or(0))));
            }
            Content::Struct(b) | Content::Array(b) => {
                let mut c = if level < self.settings.depth {
                    self.body(b, &f.children, path, level + 1)
                } else {
                    Container { stub: true, ..Container::default() }
                };
                if let Some(p) = &b.problem {
                    v.state = error(problem_text(p, path));
                }
                if b.guid_mismatch.is_some() {
                    v.warning = Some("definition GUID mismatch".into());
                }
                if !c.stub && c.children.is_empty() && c.summary.is_none() && v.state.is_none() {
                    c.summary = Some("(empty)".into());
                }
                v.container = Some(c);
            }
            Content::Block(b) => self.block(n, b, &mut v, path, level),
            Content::Heading => {}
        }
        v
    }

    fn block(&mut self, n: &'s h5_meta::Node<'s>, b: &'s Block<'s>, v: &mut FieldView, path: &str, level: u32) {
        v.short_type = short_type(FieldType::Block64, Some(b.count));
        let labels: Vec<String> = b.entries.iter().enumerate().map(|(i, e)| entry_label(e, i)).collect();
        let mut c = Container { count: Some(b.count), labels: labels.clone(), ..Container::default() };
        if b.guid_mismatch.is_some() {
            v.warning = Some("definition GUID mismatch".into());
        }
        if let Some(p) = &b.problem {
            v.state = error(problem_text(p, path));
        } else if b.count == 0 {
            v.state = state(marker::EMPTY, "empty");
        } else {
            let mut summary = label_list(&labels);
            if level < self.settings.depth
                && let Some(first) = b.entries.first()
            {
                summary.push_str(&format!("  showing [0] {}", labels[0]));
                c.index = Some(0);
                let inner = self.body(first, &n.field.children, &format!("{path}[0]"), level + 1);
                c.children = inner.children;
                c.not_read = inner.not_read;
                if let Some(p) = &first.problem {
                    v.state = error(problem_text(p, &format!("{path}[0]")));
                }
            } else {
                c.stub = true;
            }
            c.summary = Some(summary);
        }
        v.container = Some(c);
    }

    fn value(&self, n: &h5_meta::Node<'_>, value: &Value, v: &mut FieldView) {
        let f = n.field;
        let t = match f.kind {
            FieldKind::Known(t) => t,
            FieldKind::Unknown(_) => return,
        };
        let real = |x: f32| x.to_string();
        v.value = Some(match value {
            Value::Int(i) => plain(i.to_string(), json!(i)),
            Value::Uint(u) => plain(u.to_string(), json!(u)),
            Value::Real(x) => plain(real(*x), json!(x)),
            Value::Reals(xs)
                if matches!(t, FieldType::RealBounds | FieldType::AngleBounds | FieldType::FractionBounds) =>
            {
                plain(format!("{}..{}", real(xs[0]), real(xs[1])), json!({ "min": xs[0], "max": xs[1] }))
            }
            Value::Reals(xs) => {
                let parts: Vec<String> = xs.iter().map(|x| real(*x)).collect();
                plain(format!("({})", parts.join(", ")), json!(xs))
            }
            Value::Ints(is) if t == FieldType::ShortIntegerBounds => {
                plain(format!("{}..{}", is[0], is[1]), json!({ "min": is[0], "max": is[1] }))
            }
            Value::Ints(is) => {
                let parts: Vec<String> = is.iter().map(i64::to_string).collect();
                plain(format!("({})", parts.join(", ")), json!(is))
            }
            Value::Enum(_) => return self.enum_value(n, v),
            Value::Flags(raw) => return self.flags_value(n, *raw, v),
            Value::BlockIndex(i) => return self.block_index(n, *i, v),
            Value::Color(c) => {
                let [b, g, r, a] = c.to_le_bytes();
                match t {
                    FieldType::ArgbColor => plain(format!("({a}, {r}, {g}, {b})"), json!([a, r, g, b])),
                    _ => plain(format!("({r}, {g}, {b})"), json!([r, g, b])),
                }
            }
            Value::String(s) => plain(format!("{s:?}"), json!(s)),
            Value::StringId(s) => match (&s.text, s.hash) {
                (Some(text), _) => plain(text.clone(), json!(text)),
                (None, 0) => {
                    v.state = state(marker::NONE, "none");
                    return;
                }
                (None, h) => {
                    v.state = state(marker::UNRESOLVED, "unresolved");
                    plain(format!("0x{h:08X}"), json!(format!("0x{h:08X}")))
                }
            },
            Value::Tag(code) => plain(code.to_string(), json!(code.to_string())),
            Value::TagRef(r) => return self.tag_ref(f, r, v),
            Value::Data(d) if d.size == 0 => {
                v.state = state(marker::EMPTY, "empty");
                return;
            }
            Value::Data(d) => plain(format!("({} bytes)", d.size), json!(d.size)),
            Value::Resource(r) => match r.index.and_then(|k| self.lookup.resource(k)) {
                Some(res) => {
                    let place = if res.stored { "not loaded" } else { "not stored in this module" };
                    v.state = state(
                        format!("({}, {}, {place})", res.kind, format_size(res.size)),
                        if res.stored { "not_loaded" } else { "not_stored" },
                    );
                    return;
                }
                None if r.index.is_some() => {
                    v.state = state("(resource, not loaded)", "not_loaded");
                    return;
                }
                None => {
                    v.state = state(marker::NONE, "none");
                    return;
                }
            },
            Value::Padding => plain(format!("({} bytes)", f.size().unwrap_or(0)), json!(f.size().unwrap_or(0))),
        });
    }

    fn enum_value(&self, n: &h5_meta::Node<'_>, v: &mut FieldView) {
        let Content::Value(Value::Enum(value)) = n.content else { return };
        let visible = self.visible(&n.field.options);
        let mut json = Map::from_iter([("value".into(), json!(value))]);
        let text = match n.enum_option() {
            Some(EnumOption::Named(o)) => {
                json.insert("label".into(), json!(o.display));
                format!("{value} {}", o.display)
            }
            Some(EnumOption::Missing { value, defined }) => {
                v.state = state(format!("(no option {value}; {defined} defined)"), "no_option");
                value.to_string()
            }
            _ => {
                v.state = state(marker::NO_OPTIONS, "no_options");
                value.to_string()
            }
        };
        v.value = Some(Shown { text, json });
        let names: Vec<String> = visible.iter().map(|(_, o)| o.display.clone()).collect();
        v.options = match self.settings.options {
            Options::None => None,
            _ if names.is_empty() => None,
            Options::All => Some((format!("{{{}}}", names.join("|")), names)),
            Options::Auto if names.len() <= AUTO_ENUM_OPTIONS => Some((format!("{{{}}}", names.join("|")), names)),
            Options::Auto => {
                let others = names.len() - usize::from(matches!(n.enum_option(), Some(EnumOption::Named(_))));
                Some((format!("(+{others} options)"), names))
            }
        };
    }

    fn flags_value(&self, n: &h5_meta::Node<'_>, raw: u64, v: &mut FieldView) {
        let bits = n.flag_bits().expect("a flags value");
        let width = n.field.size().unwrap_or(4) as usize * 2;
        let set: Vec<String> = bits.set.iter().map(|(_, o)| o.display.clone()).collect();
        let mut json = Map::from_iter([("raw".into(), json!(raw)), ("set".into(), json!(set))]);
        let mut text = format!("0x{raw:0width$X}");
        if raw == 0 {
            v.state = state(marker::NONE_SET, "none_set");
        } else if !set.is_empty() {
            text.push_str(&format!(" {{{}}}", set.join(", ")));
        }
        if !bits.unnamed.is_empty() {
            let list: Vec<String> = bits.unnamed.iter().map(u32::to_string).collect();
            text.push_str(&format!(" unnamed bits: {}", list.join(", ")));
            json.insert("unnamed_bits".into(), json!(bits.unnamed));
        }
        if n.field.options.is_empty() {
            v.state = state(marker::NO_OPTIONS, "no_options");
        }
        v.value = Some(Shown { text, json });
        let visible = self.visible(&n.field.options);
        let names: Vec<String> = visible.iter().map(|(_, o)| o.display.clone()).collect();
        let unset = visible.iter().filter(|(i, _)| raw & (1u64 << i) == 0).count();
        v.options = match self.settings.options {
            Options::None => None,
            _ if names.is_empty() => None,
            Options::All => Some((format!("{{{}}}", names.join(", ")), names)),
            Options::Auto if unset > 0 => Some((format!("(+{unset} options)"), names)),
            Options::Auto => None,
        };
    }

    fn block_index(&self, n: &h5_meta::Node<'_>, i: i64, v: &mut FieldView) {
        let mut json = Map::from_iter([("value".into(), json!(i))]);
        if i < 0 {
            v.value = Some(Shown { text: i.to_string(), json });
            v.state = state(marker::NONE, "none");
            return;
        }
        let bodies: Vec<&Body<'_>> = self.scopes.iter().map(|(b, _)| *b).collect();
        let target = target_block(n.field, &bodies).and_then(|t| {
            let scope = self.scopes.iter().rev().find(|(b, _)| b.children.iter().any(|c| std::ptr::eq(c, t)))?;
            Some((t, join(&scope.1, t.name.unwrap_or_default())))
        });
        let text = match target {
            Some((t, p)) => {
                let Content::Block(b) = &t.content else { unreachable!("target_block returns blocks") };
                if i as u64 >= b.count as u64 {
                    v.state = state(format!("(out of range; block has {})", b.count), "out_of_range");
                    i.to_string()
                } else {
                    let label = b.entries.get(i as usize).map(|e| entry_label(e, i as usize)).unwrap_or_default();
                    json.insert("target".into(), json!({ "path": format!("{p}[{i}]"), "label": label }));
                    format!("{i} → {p}[{i}] {label}")
                }
            }
            None => {
                v.state = state("(target block unknown)", "no_target");
                i.to_string()
            }
        };
        v.value = Some(Shown { text, json });
    }

    fn tag_ref(&self, f: &Field, r: &h5_meta::TagRef, v: &mut FieldView) {
        let valid: Vec<String> = f
            .attr("validtags")
            .map(|s| s.split([',', ' ']).filter(|c| !c.is_empty()).map(str::to_owned).collect())
            .unwrap_or_default();
        let classes = format!("[{}]", valid.join(", "));
        if r.field.is_null() {
            // `(none) [proj]` (PRD §8): the state comes first.
            v.state = Some(State { text: String::new(), key: "none", error: false });
            let target = json!({ "valid_classes": valid });
            v.value = Some(Shown {
                text: format!("{} {classes}", marker::NONE),
                json: Map::from_iter([("target".into(), target)]),
            });
            return;
        }
        let id = r.field.global_id;
        let indexed = self.lookup.tag_name(id);
        let name = indexed
            .clone()
            .or_else(|| r.listed_name.as_ref().map(|n| format!("{n}.{}", self.lookup.class_name(r.field.class_code))));
        let mut text = match &name {
            Some(n) => format!("{n}  {id} {classes}"),
            None => format!("{id} [{}]", r.field.class_code),
        };
        if indexed.is_none() {
            v.state = state(marker::NOT_IN_INDEX, "not_found");
            if name.is_none() {
                text = format!("{id} {classes}");
            }
        }
        let target = json!({ "name": name, "id": id.to_string(), "valid_classes": valid });
        v.value = Some(Shown { text, json: Map::from_iter([("target".into(), target)]) });
    }
}

/// An entry's label (PRD §6.4.4): its labelling field's value; tag references
/// by file name (decisions D36); else the entry index.
pub(crate) fn entry_label(e: &Body<'_>, i: usize) -> String {
    let text = e.label.and_then(|l| match &e.children[l].content {
        Content::Value(Value::StringId(s)) => s.text.clone(),
        Content::Value(Value::String(s)) => Some(s.clone()),
        Content::Value(Value::TagRef(r)) if !r.field.is_null() => {
            r.listed_name.as_deref().map(|n| file_name(n).to_owned())
        }
        _ => None,
    });
    text.filter(|t| !t.is_empty()).unwrap_or_else(|| i.to_string())
}

/// The view of a read tag's main struct (or of the fields at `path`).
pub fn view_body<'s>(
    body: &'s Body<'s>,
    fields: &'s [Field],
    path: &str,
    settings: ViewSettings,
    lookup: &dyn Lookup,
) -> Vec<ViewNode> {
    let mut c = Converter { settings, lookup, scopes: Vec::new() };
    c.body(body, fields, path, 0).children
}

/// The view of what a path selected (PRD §7.3): `### <path>  (path)`, then the
/// selected fields. With several selections (`[*]`), each value is named by its
/// full path, and each struct or entry gets its own heading.
pub fn view_selection(
    sels: &[Selected<'_, '_>],
    requested: &str,
    settings: ViewSettings,
    lookup: &dyn Lookup,
) -> Vec<ViewNode> {
    let mut out =
        vec![ViewNode::Heading { kind: HeadingKind::Target, title: format!("{requested}  (path)"), text: None }];
    let several = sels.len() > 1;
    for s in sels {
        let mut c = Converter { settings, lookup, scopes: s.scopes.iter().map(|(b, p)| (*b, p.clone())).collect() };
        let body = match (s.entry, &s.node.content) {
            (Some(i), Content::Block(b)) => b.entries.get(i as usize),
            (None, Content::Struct(b) | Content::Array(b)) if !s.node.field.is_mapping_function() => Some(b),
            _ => None,
        };
        match body {
            Some(body) => {
                if several {
                    out.push(ViewNode::Heading { kind: HeadingKind::Target, title: s.path.clone(), text: None });
                }
                out.extend(c.body(body, &s.node.field.children, &s.path, 0).children);
            }
            None => {
                let mut v = c.field(s.node, &s.path, 0);
                if v.container.is_none() {
                    v.name = s.path.clone();
                }
                out.push(ViewNode::Field(Box::new(v)));
            }
        }
    }
    out
}

/// A struct of a read tag: its body, definition fields, path, and the structs around it.
pub struct StructAt<'s> {
    pub body: &'s Body<'s>,
    pub fields: &'s [Field],
    pub path: String,
    pub scopes: Vec<(&'s Body<'s>, String)>,
}

/// For `[*]` reads (AGENT_GUIDE §4.4): moves each field's options out of its
/// lines into one legend line per field (path without indices), listing every
/// option. JSON keeps the options on each field.
pub fn into_legend(nodes: &mut [ViewNode]) -> Vec<String> {
    fn visit(nodes: &mut [ViewNode], seen: &mut Vec<String>, out: &mut Vec<String>) {
        for n in nodes {
            let ViewNode::Field(f) = n else { continue };
            if let Some((_, list)) = &f.options {
                let key: String = {
                    let mut k = String::new();
                    let mut skip = false;
                    for c in f.path.chars() {
                        match c {
                            '[' => skip = true,
                            ']' => skip = false,
                            _ if !skip => k.push(c),
                            _ => {}
                        }
                    }
                    k
                };
                if !seen.contains(&key) {
                    let sep = if f.short_type.starts_with("flags") { ", " } else { "|" };
                    out.push(format!("{key}  {{{}}}", list.join(sep)));
                    seen.push(key);
                }
                f.in_legend = true;
            }
            if let Some(c) = f.container.as_mut() {
                visit(&mut c.children, seen, out);
            }
        }
    }
    let (mut seen, mut out) = (Vec::new(), Vec::new());
    visit(nodes, &mut seen, &mut out);
    out
}

/// The view of a section (PRD §6.4.3): `### <heading>  (section)`, then fields
/// `start + 1 .. end` of the struct `at`.
pub fn view_section(
    at: &StructAt<'_>,
    (start, end): (usize, usize),
    heading: &str,
    settings: ViewSettings,
    lookup: &dyn Lookup,
) -> Vec<ViewNode> {
    let StructAt { body, fields, path, scopes } = at;
    let end = end.min(body.children.len());
    let slice = Body { children: body.children[(start + 1).min(end)..end].to_vec(), ..Body::default() };
    let mut c = Converter { settings, lookup, scopes: scopes.to_vec() };
    let mut out =
        vec![ViewNode::Heading { kind: HeadingKind::Target, title: format!("{heading}  (section)"), text: None }];
    out.extend(c.body(&slice, &fields[(start + 1).min(end)..end], path, 0).children);
    out
}

#[cfg(test)]
#[path = "tag_view_tests.rs"]
mod tests;
