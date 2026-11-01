//! One definition file (PRD §5.3): `<plugin>` with one element per field; the
//! element name is the field type. Kept close to the XML: raw names, raw
//! attributes, options and explanation text, plus each name parsed (`name`).

use quick_xml::events::{BytesRef, BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

use crate::field_type::{FieldType, Size};
use crate::name::Name;

/// A field's type: one the table knows, or the element name as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Known(FieldType),
    /// Not in the field type table: reading stops at it (PRD §6.4.4).
    Unknown(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub kind: FieldKind,
    /// The `name` attribute as written, with markers, units and help.
    pub raw_name: String,
    pub name: Name,
    /// Every attribute but `name`, keys as written.
    attributes: Vec<(String, String)>,
    /// `<item Name="...">` children, in order: an option's value (enum) or bit
    /// number (flags) is its position. Item names use the field name syntax.
    pub options: Vec<Name>,
    /// Fields of a struct, array or tag block entry.
    pub children: Vec<Field>,
    /// An explanation's text content, as written (line breaks kept).
    pub text: Option<String>,
    /// The field's step in a path: its display name, unique among its
    /// siblings (`name#2`, PRD §6.4.2). `None` for explanations and customs.
    /// Set once, when the definition is parsed.
    pub step: Option<String>,
}

impl Field {
    /// Attribute `key`, matched case-insensitively (PRD §5.3).
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attributes.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
    }

    /// Bytes the field takes in its struct; `None` for an unknown type or a
    /// size that can't be computed (padding without a `length`).
    pub fn size(&self) -> Option<u32> {
        let FieldKind::Known(t) = &self.kind else { return None };
        match t.size() {
            Size::Fixed(n) => Some(n),
            Size::Children => children_size(&self.children),
            Size::Length => self.attr("length")?.trim().parse().ok(),
        }
    }

    /// For a tag block field: the size of one entry (the sum of its children).
    pub fn entry_size(&self) -> Option<u32> {
        children_size(&self.children)
    }

    fn is(&self, t: FieldType) -> bool {
        self.kind == FieldKind::Known(t)
    }

    /// The `srcguid` as the 16 bytes a tag stores for this struct type: a
    /// Windows GUID, its first three groups little-endian (format-notes "GUIDs").
    pub fn guid(&self) -> Option<[u8; 16]> {
        let hex: Vec<u8> = self.attr("srcguid")?.bytes().filter(|&b| b != b'-').collect();
        if hex.len() != 32 {
            return None;
        }
        let mut g = [0u8; 16];
        for (i, b) in g.iter_mut().enumerate() {
            *b = u8::from_str_radix(std::str::from_utf8(&hex[i * 2..i * 2 + 2]).ok()?, 16).ok()?;
        }
        g[0..4].reverse();
        g[4..6].reverse();
        g[6..8].reverse();
        Some(g)
    }

    fn has_srcguid(&self, guid: &str) -> bool {
        self.attr("srcguid").is_some_and(|g| g.eq_ignore_ascii_case(guid))
    }

    /// The `vtable space` field and the internal struct at the top of every
    /// definition: never shown, even with `show_hidden` (PRD §5.3, §7.2).
    pub fn is_internal(&self) -> bool {
        self.has_srcguid(INTERNAL_STRUCT_GUID)
            || (self.is(FieldType::Int64Integer) && self.name.display == "vtable space")
    }

    /// A mapping-function struct: shown as one line (PRD §5.3, §7.2).
    pub fn is_mapping_function(&self) -> bool {
        self.is(FieldType::Struct) && self.has_srcguid(MAPPING_FUNCTION_GUID)
    }
}

/// The "internal struct" (global tag ID + local handle), PRD §5.3.
pub const INTERNAL_STRUCT_GUID: &str = "151d0146-479c-181f-3970-ec869043bc33";
/// The mapping-function struct (529 uses), PRD §5.3.
pub const MAPPING_FUNCTION_GUID: &str = "5a14a98f-4484-53fa-d468-10809e45f10a";

/// What a `_field_custom` is (docs/format-notes.md "Names").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Custom {
    /// All uppercase (`OBJECT`, `$$$ UNIT $$$` → `UNIT`): at the top level, a class section.
    ClassSection(String),
    /// Any other name: a heading that groups the fields after it (`Outlining`).
    Group(String),
    /// An editor control: for the next field (same name), a tool button
    /// (`import …`, `show …`), or nameless. Not shown.
    Widget,
}

/// The role of `fields[i]` if it is a `_field_custom`. Uses the next sibling,
/// so it takes the whole list.
pub fn custom_role(fields: &[Field], i: usize) -> Option<Custom> {
    let f = fields.get(i).filter(|f| f.is(FieldType::Custom))?;
    let label = f.raw_name.trim_matches(|c: char| c == '$' || c == ' ');
    let next_has_same_name = fields.get(i + 1).is_some_and(|n| n.raw_name == f.raw_name);
    let tool = ["import", "show"].iter().any(|w| {
        f.raw_name.get(..w.len()).is_some_and(|p| p.eq_ignore_ascii_case(w))
            && f.raw_name[w.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric())
    });
    Some(if label.is_empty() || next_has_same_name || tool {
        Custom::Widget
    } else if label.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == ' ' || c == '_') {
        Custom::ClassSection(label.to_owned())
    } else {
        Custom::Group(f.name.display.clone())
    })
}

/// Each field's name as a path step (PRD §6.4.2): the display name, with
/// `#2`, `#3` added to later siblings of the same name (compared
/// case-insensitively, like paths). `None` for explanations and customs,
/// which a path can't name.
pub fn unique_names(fields: &[Field]) -> Vec<Option<String>> {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    fields
        .iter()
        .map(|f| {
            if f.is(FieldType::Explanation) || f.is(FieldType::Custom) {
                return None;
            }
            let n = seen.entry(f.name.display.to_lowercase()).or_default();
            *n += 1;
            Some(if *n == 1 { f.name.display.clone() } else { format!("{}#{n}", f.name.display) })
        })
        .collect()
}

/// Sets every field's path step, at every depth.
fn set_steps(fields: &mut [Field]) {
    let steps = unique_names(fields);
    for (f, step) in fields.iter_mut().zip(steps) {
        f.step = step;
        set_steps(&mut f.children);
    }
}

fn children_size(fields: &[Field]) -> Option<u32> {
    fields.iter().try_fold(0u32, |sum, f| sum.checked_add(f.size()?))
}

/// A parsed definition: the top-level fields of its class's main struct.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Definition {
    pub fields: Vec<Field>,
}

#[derive(Debug, thiserror::Error)]
pub enum DefinitionError {
    #[error("XML: {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("XML attribute: {0}")]
    Attribute(#[from] quick_xml::events::attributes::AttrError),
    #[error("not a definition: the root element is <{0}>, expected <plugin>")]
    NotAPlugin(String),
    #[error("the XML ends inside <{0}>")]
    Unclosed(String),
    #[error("unknown entity &{0};")]
    Entity(String),
}

impl Definition {
    pub fn size(&self) -> Option<u32> {
        children_size(&self.fields)
    }

    pub fn parse(xml: &str) -> Result<Definition, DefinitionError> {
        let mut reader = Reader::from_str(xml);
        // Open elements: the `<plugin>` root, then fields and items.
        let mut stack: Vec<Open> = Vec::new();
        let mut root: Option<Vec<Field>> = None;
        loop {
            match reader.read_event()? {
                Event::Start(e) => match stack.last_mut() {
                    None => {
                        expect_plugin(&e)?;
                        stack.push(Open::Plugin(Vec::new()));
                    }
                    Some(_) => stack.push(open(&e)?),
                },
                Event::Empty(e) => match stack.last_mut() {
                    None => {
                        expect_plugin(&e)?;
                        root = Some(Vec::new());
                    }
                    Some(parent) => {
                        let child = open(&e)?;
                        parent.add(child);
                    }
                },
                Event::End(_) => {
                    let Some(done) = stack.pop() else { continue };
                    match (done, stack.last_mut()) {
                        (Open::Plugin(fields), None) => root = Some(fields),
                        (done, Some(parent)) => parent.add(done),
                        (_, None) => unreachable!("only <plugin> is opened at the root"),
                    }
                }
                Event::Text(t) => push_text(&mut stack, &t.xml10_content()),
                Event::GeneralRef(r) => push_text(&mut stack, &entity(&r)?),
                Event::Eof => break,
                _ => {} // comments, declarations, CDATA (unused)
            }
        }
        match (root, stack.last()) {
            (Some(mut fields), None) => {
                set_steps(&mut fields);
                Ok(Definition { fields })
            }
            (_, Some(open)) => Err(DefinitionError::Unclosed(open.element().to_owned())),
            (None, None) => Err(DefinitionError::NotAPlugin(String::new())),
        }
    }
}

/// An element being read.
enum Open {
    Plugin(Vec<Field>),
    Field(Box<Field>),
    /// An option; its `Name` attribute.
    Item(String),
}

impl Open {
    fn element(&self) -> &str {
        match self {
            Open::Plugin(_) => "plugin",
            Open::Field(f) => match &f.kind {
                FieldKind::Known(t) => t.xml_name(),
                FieldKind::Unknown(name) => name,
            },
            Open::Item(_) => "item",
        }
    }

    /// Adds a finished child element.
    fn add(&mut self, child: Open) {
        let fields = match self {
            Open::Plugin(fields) => fields,
            Open::Field(f) => match child {
                Open::Item(name) => return f.options.push(Name::parse(&name)),
                _ => &mut f.children,
            },
            Open::Item(_) => return,
        };
        if let Open::Field(f) = child {
            fields.push(*f);
        }
    }
}

fn expect_plugin(e: &BytesStart<'_>) -> Result<(), DefinitionError> {
    let name = e.name().as_ref().to_owned();
    if name == "plugin" { Ok(()) } else { Err(DefinitionError::NotAPlugin(name)) }
}

/// A field or item element, from its name and attributes.
fn open(e: &BytesStart<'_>) -> Result<Open, DefinitionError> {
    let element = e.name().as_ref().to_owned();
    let mut name = String::new();
    let mut attributes = Vec::new();
    for a in e.attributes() {
        let a = a?;
        let key = a.key.as_ref().to_owned();
        let value = a.normalized_value(XmlVersion::Implicit1_0)?.into_owned();
        if key.eq_ignore_ascii_case("name") {
            name = value;
        } else {
            attributes.push((key, value));
        }
    }
    if element == "item" {
        return Ok(Open::Item(name));
    }
    let kind = FieldType::from_xml(&element).map_or(FieldKind::Unknown(element), FieldKind::Known);
    Ok(Open::Field(Box::new(Field {
        kind,
        name: Name::parse(&name),
        raw_name: name,
        attributes,
        options: Vec::new(),
        children: Vec::new(),
        text: None,
        step: None,
    })))
}

/// Text inside a field (an explanation's content). Whitespace between child
/// elements is not text.
fn push_text(stack: &mut [Open], text: &str) {
    let Some(Open::Field(f)) = stack.last_mut() else { return };
    match &mut f.text {
        Some(t) => t.push_str(text),
        None if !text.trim().is_empty() => f.text = Some(text.to_owned()),
        None => {}
    }
}

/// `&amp;`, `&lt;`, `&#233;`, ...
fn entity(r: &BytesRef<'_>) -> Result<String, DefinitionError> {
    if let Some(c) = r.resolve_char_ref()? {
        return Ok(c.to_string());
    }
    let name = r.xml10_content();
    quick_xml::escape::resolve_predefined_entity(&name)
        .map(str::to_owned)
        .ok_or_else(|| DefinitionError::Entity(name.into_owned()))
}

#[cfg(test)]
#[path = "definition_tests.rs"]
mod tests;
