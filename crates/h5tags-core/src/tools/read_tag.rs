//! `read_tag` (PRD §6.4): a tag's fields read with its class's definition.

use h5_defs::definition::Definition;
use h5_meta::{Content, find_sections, parse_path, read_tag, select};
use serde::Deserialize;
use serde_json::{Map, json};

use super::Tool;
use super::args::{self, BuildArg};
use super::present::{self, GUARD_OUTLINE_DEPTH};
use crate::catalog::{Build, Copy, Tag};
use crate::def_view::outline_tag;
use crate::output::{Output, ToolError, format_size};
use crate::session::Session;
use crate::stored::{Identity, copy_identity, load_definition, read_stored_tag};
use crate::tag_view::{
    BuildLookup, Detail, Lookup, Options, ResourceInfo, StructAt, ViewSettings, into_legend, view_body, view_section,
    view_selection,
};
use crate::view::TagView;

fn default_depth() -> u32 {
    2
}

/// Read a tag's fields with its class's definition: values, options, units
/// and states. Large tags (scnr, effe, vehi, bipd, char, gint, unit, weap):
/// call outline first, then read path= or section=.
#[derive(Clone, Debug, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct ReadTag {
    /// The tag: global ID, tag path or display name.
    pub tag: String,
    /// Only this field: `barrels[0].firing noise`, `barrels[*].rounds per second`.
    #[arg(long)]
    pub path: Option<String>,
    /// Only this section: a class section, explanation or group heading.
    #[arg(long)]
    pub section: Option<String>,
    /// Nested levels of structs and tag blocks expanded below what is read.
    #[arg(long, default_value_t = 2)]
    #[serde(default = "default_depth")]
    pub depth: u32,
    /// brief = paths and values only (no units, options or headings); normal =
    /// + units, ranges, options, headings; full = + help, offsets, markers.
    #[arg(long, value_enum, default_value_t)]
    #[serde(default)]
    pub detail: Detail,
    /// auto = small enums list all options; all; none.
    #[arg(long, value_enum, default_value_t)]
    #[serde(default)]
    pub options: Options,
    /// Also show padding and fields the definition marks internal (`!`).
    #[arg(long)]
    #[serde(default)]
    pub show_hidden: bool,
    /// A whole read over the size limit returns the outline instead; true
    /// returns everything (with a size warning). Use only when the outline
    /// and path=/section= reads can't answer.
    #[arg(long)]
    #[serde(default)]
    pub confirm_large: bool,
    /// Read the copy in the module whose path contains this.
    #[arg(long)]
    pub module: Option<String>,
    #[command(flatten)]
    #[serde(flatten)]
    pub build: BuildArg,
}

/// The tag line (PRD §7.3) and its JSON form.
pub(super) fn title(b: &Build, tag: &Tag, copy: Copy, identity: Identity, detail: Detail, view: &mut TagView) {
    let name = b.display_name(tag);
    let mut line = format!("# {} {name}  {}  build={}", tag.class, tag.global_id, b.name);
    let mut json = Map::new();
    json.insert("class".into(), json!(tag.class.to_string()));
    json.insert("name".into(), json!(name));
    json.insert("id".into(), json!(tag.global_id.to_string()));
    json.insert("build".into(), json!(b.name));
    let module = &b.module(copy).rel_path;
    let n = tag.copies.len();
    match identity {
        Identity::Single => {}
        Identity::Identical => line.push_str(&format!("  copies: {n} (identical)")),
        Identity::Differ => line.push_str(&format!("  copies: {n} (differ; reading {module}; use module= to choose)")),
    }
    if identity != Identity::Single {
        json.insert(
            "copies".into(),
            json!({ "count": n, "identical": identity == Identity::Identical, "reading": module }),
        );
    }
    let lookup = BuildLookup { build: b, copy };
    let resources: Vec<ResourceInfo> =
        (0..b.entry(copy).resources.len() as u32).filter_map(|k| lookup.resource(k)).collect();
    if !resources.is_empty() {
        let total: u64 = resources.iter().map(|r| r.size).sum();
        line.push_str(&format!("  resources: {} ({})", resources.len(), format_size(total)));
        json.insert("resources".into(), json!({ "count": resources.len(), "size": total }));
        if detail == Detail::Full {
            for (i, r) in resources.iter().enumerate() {
                view.title_lines.push(format!("resource [{i}]  {}  {}", r.kind, format_size(r.size)));
            }
        }
    }
    view.title = line;
    view.title_json = json;
}

impl Tool for ReadTag {
    const NAME: &'static str = "read_tag";

    fn run(&self, s: &mut Session) -> Result<Output, ToolError> {
        if self.path.is_some() && self.section.is_some() {
            return Err(ToolError::new("path= and section= can't be used together").hint("pass one of them"));
        }
        let (limit, guid_check) = (s.large_read_tokens, s.guid_check);
        let t = args::tag_copy(s, &self.tag, &self.build, self.module.as_deref())?;
        let (b, copy) = (t.build, t.copy);
        let identity = copy_identity(b, t.tag).map_err(present::read_error)?;
        let settings = ViewSettings {
            depth: self.depth,
            detail: self.detail,
            options: self.options,
            show_hidden: self.show_hidden,
            guid_check,
        };
        let mut view = TagView { brief: self.detail == Detail::Brief, ..TagView::default() };
        title(b, t.tag, copy, identity, self.detail, &mut view);
        let Some(def) = load_definition(b, t.tag).map_err(present::definition_error)? else {
            view.title_lines.push(present::no_definition(&b.class_name(t.tag.class)));
            view.title_json.insert("state".into(), json!("no_definition"));
            return Ok(Output::Tag(view));
        };
        let stored = read_stored_tag(b, copy).map_err(present::read_error)?;
        let read = read_tag(&def, &stored.header, &stored.tag_data);
        if let Some(line) = present::divergences(&read.divergences) {
            view.title_lines.push(line);
        }
        let lookup = BuildLookup { build: b, copy };
        match (&self.path, &self.section) {
            (Some(path), _) => {
                let p = parse_path(&def.fields, path).map_err(present::path_error)?;
                let sels = select(&read.root, &p).map_err(present::path_error)?;
                let selection = view_selection(&sels, &p.display(&def.fields), settings, &lookup);
                view.nodes = selection.nodes;
                view.title_lines.extend(selection.notes);
                if sels.len() > 1 {
                    view.legend = into_legend(&mut view.nodes);
                }
            }
            (None, Some(heading)) => view.nodes = section(&def, &read.root, heading, settings, &lookup)?,
            (None, None) => {
                if let Some(p) = &read.root.problem {
                    view.title_lines.push(present::main_struct_problem(p));
                }
                view.nodes = view_body(&read.root, &def.fields, "", settings, &lookup);
                let outline = || outline_tag(&read.root, &def.fields, GUARD_OUTLINE_DEPTH);
                view = present::size_guard(view, limit, self.confirm_large, outline);
            }
        }
        Ok(Output::Tag(view))
    }
}

/// `section=`: the one section with that heading (PRD §6.4.3).
fn section(
    def: &Definition,
    root: &h5_meta::Body<'_>,
    heading: &str,
    settings: ViewSettings,
    lookup: &dyn Lookup,
) -> Result<Vec<crate::view::ViewNode>, ToolError> {
    let found = find_sections(&def.fields, heading);
    let [sec] = found.as_slice() else {
        return Err(present::section_error(heading, &found, &def.fields));
    };
    // The struct holding the section: the main struct, or the one at its parent path.
    let at = if sec.parent.steps.is_empty() {
        StructAt { body: root, fields: &def.fields, path: String::new(), scopes: Vec::new() }
    } else {
        let sel = select(root, &sec.parent).map_err(present::path_error)?;
        let s = sel
            .into_iter()
            .next()
            .ok_or_else(|| present::path_error(h5_meta::PathError::NotRead { at: sec.parent.display(&def.fields) }))?;
        let body = match (&s.node.content, s.entry) {
            (Content::Struct(b) | Content::Array(b), _) => b,
            (Content::Block(b), _) => b
                .entries
                .first()
                .ok_or_else(|| present::path_error(h5_meta::PathError::Empty { at: s.path.clone() }))?,
            _ => return Err(present::path_error(h5_meta::PathError::NoFields { at: s.path })),
        };
        let path = if matches!(s.node.content, Content::Block(_)) { format!("{}[0]", s.path) } else { s.path.clone() };
        StructAt { body, fields: &s.node.field.children, path, scopes: s.scopes }
    };
    Ok(view_section(&at, (sec.start, sec.end), &sec.heading, settings, lookup))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::Format;
    use crate::testdeploy::{BULLET, SecondCopy, TestDeploy, WEAPON};

    fn read(d: &mut TestDeploy, f: impl FnOnce(&mut ReadTag)) -> Result<String, ToolError> {
        let mut t = ReadTag {
            tag: WEAPON.into(),
            path: None,
            section: None,
            depth: 2,
            detail: Detail::Normal,
            options: Options::Auto,
            show_hidden: false,
            confirm_large: false,
            module: None,
            build: BuildArg::default(),
        };
        f(&mut t);
        t.run(&mut d.session).map(|o| o.render(Format::Text))
    }

    #[test]
    fn reads_the_whole_tag() {
        let mut d = TestDeploy::new("read-whole");
        let text = read(&mut d, |_| {}).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], r"# weap objects\test\test.weapon  0x00000010  build=forge  resources: 1 (19 B)"); // the resource entry: 10-byte header + 9 bytes of data
        assert_eq!(lines[1], "## WEAPON");
        assert!(lines.contains(&format!("projectile  tagref  {BULLET}  0x00401000 [proj]").as_str()), "{text}");
        assert!(lines.contains(&"count  int32  0"), "{text}");
    }

    #[test]
    fn the_tag_line_says_whether_copies_differ() {
        let mut d = TestDeploy::with("read-identical", SecondCopy::Identical);
        assert!(read(&mut d, |_| {}).unwrap().lines().next().unwrap().contains("  copies: 2 (identical)"));
        let mut d = TestDeploy::with("read-differ", SecondCopy::Changed);
        let first = read(&mut d, |_| {}).unwrap().lines().next().unwrap().to_owned();
        assert!(
            first.contains(r"copies: 2 (differ; reading any\levels\test-rtx-1.module; use module= to choose)"),
            "{first}"
        );
        // module= reads the other copy.
        let text = read(&mut d, |t| t.module = Some("11587".into())).unwrap();
        assert!(text.contains("\ncount  int32  9\n"), "{text}");
    }

    #[test]
    fn a_path_reads_one_field_or_every_entry() {
        let mut d = TestDeploy::new("read-path");
        let text = read(&mut d, |t| t.path = Some("entries[*].target".into())).unwrap();
        assert!(text.contains("\n### entries[*].target  (path)\n"), "{text}");
        assert!(text.contains("\nentries[0].target  tagref  (none) []\n"), "{text}");
        assert!(text.contains("\nentries[1].target  tagref  "), "{text}");
        let err = read(&mut d, |t| t.path = Some("projectil".into())).unwrap_err();
        assert_eq!(err.message, "no field 'projectil' in the main struct");
        assert!(err.hint.unwrap().starts_with("closest names there: projectile"));
    }

    #[test]
    fn a_section_reads_up_to_the_next_heading() {
        let mut d = TestDeploy::new("read-section");
        let text = read(&mut d, |t| t.section = Some("weapon".into())).unwrap();
        assert!(text.contains("\n### WEAPON  (section)\n"), "{text}");
        assert!(text.contains("\ncount  int32  0\n"), "{text}");
        let err = read(&mut d, |t| t.section = Some("nothing".into())).unwrap_err();
        assert_eq!(err.hint.as_deref(), Some("outline lists the headings"));
        assert!(
            read(&mut d, |t| {
                t.path = Some("count".into());
                t.section = Some("weapon".into());
            })
            .is_err()
        );
    }

    // PRD §6.4.1, the size guard: the sample weapon's whole read is about 60
    // tokens, so a limit of 10 makes it "large".

    #[test]
    fn a_large_whole_read_returns_the_outline_and_says_how_to_go_on() {
        let mut d = TestDeploy::new("read-guard");
        d.session.large_read_tokens = 10;
        let text = read(&mut d, |_| {}).unwrap();
        assert!(text.contains("\n### entries  block[2]  {0, bullet_hit}  (1 field)  path=entries\n"), "{text}");
        assert!(!text.contains("count  int32"), "no values: {text}");
        let hint = text.lines().find(|l| l.starts_with("hint: whole read ≈ ")).expect(&text);
        assert!(hint.ends_with(" lines); read a path= or section=, or repeat with confirm_large=true"), "{hint}");
    }

    #[test]
    fn confirm_large_returns_everything_with_a_warning_first() {
        let mut d = TestDeploy::new("read-guard-confirm");
        d.session.large_read_tokens = 10;
        let text = read(&mut d, |t| t.confirm_large = true).unwrap();
        let second = text.lines().nth(1).unwrap();
        assert!(second.starts_with("  warning: large result, ≈ ") && second.ends_with(" lines)"), "{text}");
        assert!(text.contains("\ncount  int32  0\n"), "{text}");
    }

    #[test]
    fn small_reads_and_path_reads_are_not_guarded() {
        let mut d = TestDeploy::new("read-guard-small");
        let text = read(&mut d, |_| {}).unwrap();
        assert!(!text.contains("warning:") && !text.contains("hint:"), "{text}");
        d.session.large_read_tokens = 1;
        let text = read(&mut d, |t| t.path = Some("entries".into())).unwrap();
        assert!(text.contains("\n  target  tagref  (none) []\n"), "{text}");
        let text = read(&mut d, |t| t.section = Some("weapon".into())).unwrap();
        assert!(text.contains("\ncount  int32  0\n"), "{text}");
    }

    #[test]
    fn a_class_without_a_definition_says_so() {
        let mut d = TestDeploy::new("read-nodef");
        let text = read(&mut d, |t| t.tag = BULLET.into()).unwrap();
        assert!(text.contains("(no definition for class 'projectile')"), "{text}");
    }
}
