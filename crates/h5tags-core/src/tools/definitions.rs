//! `get_definition`, `outline`, `search_definitions` (PRD §6.5): definitions
//! read without tag data, and the section tree of a class or a tag.

use h5_defs::definition::{Custom, Definition, Field, FieldKind, custom_role};
use h5_defs::field_type::FieldType;
use h5_meta::{find_sections, read_tag};
use serde::Deserialize;
use serde_json::json;

use super::args::{self, BuildArg, PageArgs};
use super::read_tag::title;
use super::{Tool, present};
use crate::catalog::{Build, ClassLookup};
use crate::def_view::{definition_section, definition_view, outline_definition, outline_tag};
use crate::output::{Cell, Output, Table, ToolError, col};
use crate::session::Session;
use crate::stored::{copy_identity, load_definition, load_definition_file, read_stored_tag};
use crate::tag_view::{Detail, Options, ViewSettings, join};
use crate::view::TagView;

fn default_depth() -> u32 {
    2
}

fn default_outline_depth() -> u32 {
    3
}

/// A class's definition: (class code, class name, parsed definition).
fn class_definition(b: &Build, query: &str) -> Result<(String, String, Definition), ToolError> {
    let (code, name, path) = match b.lookup_class(query) {
        ClassLookup::Found(c) => {
            let path = c.definition.clone().ok_or_else(|| {
                ToolError::new(present::no_definition(&c.name))
                    .hint("search_definitions searches the definitions that exist")
            })?;
            (c.code.to_string(), c.name.clone(), path)
        }
        ClassLookup::DefinitionOnly(d) => (d.file_code.clone(), d.class_name.clone(), d.path.clone()),
        ClassLookup::Unknown => {
            return Err(
                ToolError::new(format!("no class '{query}'")).hint("list_classes lists the classes and their codes")
            );
        }
    };
    let def = load_definition_file(&path).map_err(present::definition_error)?;
    Ok((code, name, def))
}

fn definition_title(code: &str, name: &str, view: &mut TagView) {
    view.title = format!("# {code} {name} (definition)");
    view.title_json.insert("class".into(), json!(code));
    view.title_json.insert("name".into(), json!(name));
    view.title_json.insert("definition".into(), json!(true));
}

/// The schema of a class from its definition alone: fields, types, units,
/// ranges, help, options, nesting, offsets (`detail=full`). No tag data. The
/// field dictionary: read it once instead of repeating it in every read_tag.
#[derive(Clone, Debug, clap::Args, Deserialize)]
pub struct GetDefinition {
    /// Class code or name (weap or weapon).
    pub class: String,
    /// Only this field; tag blocks are stepped into without an index.
    #[arg(long)]
    pub path: Option<String>,
    /// Only this section (a class section, explanation or group heading).
    #[arg(long)]
    pub section: Option<String>,
    /// Nested levels of structs and tag blocks expanded.
    #[arg(long, default_value_t = 2)]
    #[serde(default = "default_depth")]
    pub depth: u32,
    /// brief = names and types; normal; full = + help, offsets, markers.
    #[arg(long, value_enum, default_value_t)]
    #[serde(default)]
    pub detail: Detail,
    /// auto = enums and flags with ≤ 12 options list them; all; none.
    #[arg(long, value_enum, default_value_t)]
    #[serde(default)]
    pub options: Options,
    /// Also show padding and fields the definition marks internal (`!`).
    #[arg(long)]
    #[serde(default)]
    pub show_hidden: bool,
    #[command(flatten)]
    #[serde(flatten)]
    pub build: BuildArg,
}

impl Tool for GetDefinition {
    const NAME: &'static str = "get_definition";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        if self.path.is_some() && self.section.is_some() {
            return Err(ToolError::new("path= and section= can't be used together").hint("pass one of them"));
        }
        let b = args::build(s, &self.build)?;
        let (code, name, def) = class_definition(b, &self.class)?;
        let settings = ViewSettings {
            depth: self.depth,
            detail: self.detail,
            options: self.options,
            show_hidden: self.show_hidden,
        };
        let mut view = TagView { brief: self.detail == Detail::Brief, ..TagView::default() };
        definition_title(&code, &name, &mut view);
        view.nodes = match &self.section {
            Some(heading) => {
                let found = find_sections(&def.fields, heading);
                let [sec] = found.as_slice() else {
                    return Err(present::section_error(heading, &found, &def.fields));
                };
                definition_section(&def.fields, sec, settings)
            }
            None => definition_view(&def.fields, self.path.as_deref().unwrap_or(""), settings)
                .map_err(present::path_error)?,
        };
        Ok(Output::Tag(view))
    }
}

/// The section tree of a tag or a class: class sections, headings, structs,
/// arrays and tag blocks, each with its field path and number of direct fields;
/// for a tag, also entry counts and labels. No values. The first call for any
/// large tag. Pass exactly one of tag or class.
#[derive(Clone, Debug, clap::Args, Deserialize)]
pub struct Outline {
    /// The tag: global ID, tag path or display name.
    #[arg(long)]
    pub tag: Option<String>,
    /// Or a class code or name, for the definition's tree.
    #[arg(long)]
    pub class: Option<String>,
    /// Levels of the tree shown.
    #[arg(long, default_value_t = 3)]
    #[serde(default = "default_outline_depth")]
    pub depth: u32,
    /// For a tag: read the copy in the module whose path contains this.
    #[arg(long)]
    pub module: Option<String>,
    #[command(flatten)]
    #[serde(flatten)]
    pub build: BuildArg,
}

impl Tool for Outline {
    const NAME: &'static str = "outline";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        let mut view = TagView::default();
        match (&self.tag, &self.class) {
            (Some(tag), None) => {
                let t = args::tag_copy(s, tag, &self.build, self.module.as_deref())?;
                let identity = copy_identity(t.build, t.tag).map_err(present::read_error)?;
                title(t.build, t.tag, t.copy, identity, Detail::Normal, &mut view);
                let Some(def) = load_definition(t.build, t.tag).map_err(present::definition_error)? else {
                    view.title_lines.push(present::no_definition(&t.build.class_name(t.tag.class)));
                    return Ok(Output::Tag(view));
                };
                let stored = read_stored_tag(t.build, t.copy).map_err(present::read_error)?;
                let read = read_tag(&def, &stored.header, &stored.tag_data);
                if let Some(p) = &read.root.problem {
                    view.title_lines.push(present::main_struct_problem(p));
                }
                view.nodes = outline_tag(&read.root, &def.fields, self.depth);
            }
            (None, Some(class)) => {
                let b = args::build(s, &self.build)?;
                let (code, name, def) = class_definition(b, class)?;
                definition_title(&code, &name, &mut view);
                view.nodes = outline_definition(&def.fields, self.depth);
            }
            _ => {
                return Err(
                    ToolError::new("pass exactly one of tag= or class=").hint("outline(tag=...) or outline(class=...)")
                );
            }
        }
        Ok(Output::Tag(view))
    }
}

/// Where search_definitions looks (PRD §6.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    #[default]
    All,
    /// Field names, with their old names and labels.
    Names,
    /// Enum and flag option names.
    Options,
    /// Help text, units, explanations.
    Help,
}

/// Find which class controls something, and where: a case-insensitive search
/// of every definition's field names, option names, help text, units and
/// explanations. Each hit: class, field path (tag blocks as name[*]), field
/// type, what matched. Next: get_definition(class, path=...) or read_tag.
#[derive(Clone, Debug, clap::Args, Deserialize)]
pub struct SearchDefinitions {
    /// Text to find (part of a name, an option, or help text).
    pub query: String,
    /// Only this class (code or name).
    #[arg(long)]
    pub class: Option<String>,
    /// all, names, options or help.
    #[arg(long, value_enum, default_value_t)]
    #[serde(default)]
    pub scope: Scope,
    #[command(flatten)]
    #[serde(flatten)]
    pub build: BuildArg,
    #[command(flatten)]
    #[serde(flatten)]
    pub page: PageArgs,
}

/// One search hit: (field path, field type, what matched).
type Hit = (String, String, String);

fn search_fields(fields: &[Field], path: &str, q: &str, scope: Scope, out: &mut Vec<Hit>) {
    let has = |text: &str| text.to_lowercase().contains(q);
    let (names, options, help) = (
        matches!(scope, Scope::All | Scope::Names),
        matches!(scope, Scope::All | Scope::Options),
        matches!(scope, Scope::All | Scope::Help),
    );
    let place = |p: &str| if p.is_empty() { "(main struct)".to_owned() } else { p.to_owned() };
    for (i, f) in fields.iter().enumerate() {
        if f.is_internal() {
            continue;
        }
        match &f.kind {
            FieldKind::Known(FieldType::Explanation) => {
                let text = f.text.as_deref().unwrap_or_default();
                if help && (has(&f.name.display) || has(text)) {
                    out.push((place(path), "explanation".into(), format!("explanation '{}'", f.name.display)));
                }
                continue;
            }
            FieldKind::Known(FieldType::Custom) => {
                if let (true, Some(Custom::ClassSection(t) | Custom::Group(t))) = (names, custom_role(fields, i))
                    && has(&t)
                {
                    out.push((place(path), "heading".into(), format!("heading '{t}'")));
                }
                continue;
            }
            _ => {}
        }
        let Some(step) = &f.step else { continue };
        let p = join(path, step);
        let ty = match f.kind {
            FieldKind::Known(t) => t.xml_name().to_owned(),
            FieldKind::Unknown(ref n) => n.clone(),
        };
        let mut matched: Vec<String> = Vec::new();
        if names {
            if has(&f.name.display) {
                matched.push("name".into());
            }
            matched.extend(f.name.old_names.iter().filter(|o| has(o)).map(|o| format!("old name '{o}'")));
            matched.extend(f.name.label.iter().filter(|l| has(l)).map(|l| format!("label '{l}'")));
        }
        if options {
            let hits: Vec<&str> = f.options.iter().filter(|o| has(&o.display)).map(|o| o.display.as_str()).collect();
            if !hits.is_empty() {
                matched.push(format!("option {}", hits.join(", ")));
            }
        }
        if help {
            if f.name.help.as_deref().is_some_and(has) {
                matched.push("help".into());
            }
            if f.name.units.as_deref().is_some_and(has) {
                matched.push("units".into());
            }
        }
        if !matched.is_empty() {
            out.push((p.clone(), ty, matched.join("; ")));
        }
        let inside = if f.kind == FieldKind::Known(FieldType::Block64) { format!("{p}[*]") } else { p };
        search_fields(&f.children, &inside, q, scope, out);
    }
}

impl Tool for SearchDefinitions {
    const NAME: &'static str = "search_definitions";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        let b = args::build(s, &self.build)?;
        let q = self.query.trim().to_lowercase();
        if q.is_empty() {
            return Err(ToolError::new("the query is empty").hint("pass part of a field name, an option or help text"));
        }
        let only = self.class.as_deref().map(|c| class_definition(b, c)).transpose()?;
        let mut rows: Vec<Vec<Cell>> = Vec::new();
        let mut each = |code: &str, name: &str, def: &Definition| {
            let mut hits = Vec::new();
            search_fields(&def.fields, "", &q, self.scope, &mut hits);
            rows.extend(
                hits.into_iter()
                    .map(|(path, ty, matched)| vec![code.into(), name.into(), path.into(), ty.into(), matched.into()]),
            );
        };
        match &only {
            Some((code, name, def)) => each(code, name, def),
            None => {
                for d in b.defs.files() {
                    let def = load_definition_file(&d.path).map_err(present::definition_error)?;
                    let code = b
                        .classes
                        .values()
                        .find(|c| c.definition.as_ref() == Some(&d.path))
                        .map_or_else(|| d.file_code.clone(), |c| c.code.to_string());
                    each(&code, &d.class_name, &def);
                }
            }
        }
        let total = rows.len();
        let hints =
            (total == 0).then(|| "nothing matched; try a shorter word, or scope=all".to_owned()).into_iter().collect();
        Ok(Output::Table(Table {
            build: s.label(b),
            total,
            offset: self.page.offset,
            columns: vec![col::CLASS, col::CLASS_NAME, col::FIELD_PATH, col::FIELD_TYPE, col::MATCHED],
            rows: self.page.page(&rows, 50).to_vec(),
            wide_column: Some(2),
            hints,
            ..Table::default()
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::Format;
    use crate::testdeploy::{TestDeploy, WEAPON};

    fn text(d: &mut TestDeploy, tool: &impl Tool) -> Result<String, ToolError> {
        tool.run(&mut d.session).map(|o| o.render(Format::Text))
    }

    fn get(class: &str) -> GetDefinition {
        GetDefinition {
            class: class.into(),
            path: None,
            section: None,
            depth: 2,
            detail: Detail::Normal,
            options: Options::Auto,
            show_hidden: false,
            build: BuildArg::default(),
        }
    }

    #[test]
    fn get_definition_shows_the_schema() {
        let mut d = TestDeploy::new("def-get");
        let t = text(&mut d, &get("weapon")).unwrap();
        let l: Vec<&str> = t.lines().collect();
        assert_eq!(l[0], "# weap weapon (definition)");
        assert!(l.contains(&"projectile  tagref  [proj]"), "{t}");
        assert!(l.contains(&"### entries  block"), "{t}");
        let err = text(&mut d, &get("nothing")).unwrap_err();
        assert_eq!(err.hint.as_deref(), Some("list_classes lists the classes and their codes"));
        let t = text(&mut d, &GetDefinition { path: Some("entries[*].target".into()), ..get("weap") }).unwrap();
        assert!(t.contains("\n### entries.target  (path)\nentries.target  tagref  []\n"), "{t}");
    }

    fn outline(tag: Option<&str>, class: Option<&str>) -> Outline {
        Outline {
            tag: tag.map(Into::into),
            class: class.map(Into::into),
            depth: 3,
            module: None,
            build: BuildArg::default(),
        }
    }

    #[test]
    fn outline_of_a_class_and_of_a_tag() {
        let mut d = TestDeploy::new("def-outline");
        let t = text(&mut d, &outline(None, Some("weap"))).unwrap();
        assert!(t.contains("\n### entries  block  (1 field)  path=entries\n"), "{t}");
        let t = text(&mut d, &outline(Some(WEAPON), None)).unwrap();
        assert!(t.starts_with(r"# weap objects\test\test.weapon  0x00000010"), "{t}");
        assert!(t.contains("\n### entries  block[2]  {0, bullet_hit}  (1 field)  path=entries\n"), "{t}");
        assert!(text(&mut d, &outline(None, None)).is_err());
        assert!(text(&mut d, &outline(Some(WEAPON), Some("weap"))).is_err());
    }

    #[test]
    fn search_definitions_finds_names_and_says_what_matched() {
        let mut d = TestDeploy::new("def-search");
        let search = SearchDefinitions {
            query: "PROJ".into(),
            class: None,
            scope: Scope::All,
            build: BuildArg::default(),
            page: PageArgs::default(),
        };
        let out = search.run(&mut d.session).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out.render(Format::Json)).unwrap();
        assert_eq!(v["total"], 1);
        assert_eq!(v["rows"][0]["path"], "projectile");
        assert_eq!(v["rows"][0]["matched"], "name");
        let none = SearchDefinitions { query: "zzz".into(), ..search };
        assert!(text(&mut d, &none).unwrap().contains("hint: nothing matched"));
    }
}
