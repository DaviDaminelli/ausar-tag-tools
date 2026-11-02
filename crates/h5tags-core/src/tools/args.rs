//! The input layer: parameters shared by several tools, and turning raw
//! arguments into domain values. Bad input becomes a [`ToolError`] here, with
//! a hint; the domain (`catalog`, `filters`, `query`) never sees raw text.

use serde::Deserialize;
use serde_json::{Map, Value};

use super::present;
use h5_module::FourCc;

use crate::catalog::{Build, ClassLookup, Copy, Tag, TagLookupError, TagSpec};
use crate::field_query::{FieldFilter, Op};
use crate::filters::{Mode, ModuleFilter, NameMatcher};
use crate::output::ToolError;
use crate::query;
use crate::session::Session;

#[derive(Clone, Debug, Default, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct BuildArg {
    /// Build name (default: the first build).
    #[arg(long)]
    #[serde(default)]
    pub build: Option<String>,
}

#[derive(Clone, Debug, Default, clap::Args, Deserialize, schemars::JsonSchema)]
pub struct PageArgs {
    /// Rows to skip (paging; the result says the next offset).
    #[arg(long, default_value_t = 0)]
    #[serde(default)]
    pub offset: usize,
    /// Rows to return (default: 50 for search_tags, 100 for other lists).
    /// Bigger pages cost context: keep an eye on your budget.
    #[arg(long)]
    #[serde(default)]
    pub limit: Option<usize>,
}

impl PageArgs {
    /// Rows `offset..offset+limit` of `items`.
    pub fn page<'a, T>(&self, items: &'a [T], default_limit: usize) -> &'a [T] {
        let start = self.offset.min(items.len());
        let end = start.saturating_add(self.limit.unwrap_or(default_limit)).min(items.len());
        &items[start..end]
    }
}

pub fn build<'s>(session: &'s Session, arg: &BuildArg) -> Result<&'s Build, ToolError> {
    build_named(session, arg.build.as_deref())
}

fn build_named<'s>(session: &'s Session, name: Option<&str>) -> Result<&'s Build, ToolError> {
    session.build(name).map_err(present::unknown_build)
}

pub fn build_index(session: &Session, arg: &BuildArg) -> Result<usize, ToolError> {
    session.build_index(arg.build.as_deref()).map_err(present::unknown_build)
}

/// Resolves `[build:]<tag>` (PRD §6.1): a `build:` prefix overrides `arg`.
pub fn tag<'s>(session: &'s Session, text: &str, arg: &BuildArg) -> Result<(&'s Build, &'s Tag), ToolError> {
    let (build_name, tag_text) = match text.split_once(':') {
        Some((prefix, rest)) if session.has_build(prefix) => (Some(prefix), rest),
        _ => (arg.build.as_deref(), text),
    };
    let build = build_named(session, build_name)?;
    let spec = TagSpec::parse(tag_text).map_err(|e| ToolError::new(e).hint("global IDs look like 0x00A1B2C3"))?;
    match build.find_tag(&spec) {
        Ok(tag) => Ok((build, tag)),
        Err(TagLookupError::NotFound) => {
            Err(ToolError::new(format!("no tag '{}' in build '{}'", tag_text.trim(), build.name))
                .hint("search_tags finds tags by part of their path"))
        }
        Err(TagLookupError::Ambiguous(tags)) => Err(present::ambiguous_tag(build, tag_text.trim(), &tags)),
    }
}

/// A tag and the copy of it to use.
pub struct TagCopy<'s> {
    pub build: &'s Build,
    pub tag: &'s Tag,
    pub copy: Copy,
    /// Set when the tag has other copies (they can be chosen with `module=`).
    pub other_copies_hint: Option<String>,
}

/// Resolves `[build:]<tag>` and picks its copy (PRD §6.4.6): the first one, or
/// the one in the module matching `module`.
pub fn tag_copy<'s>(
    session: &'s Session,
    text: &str,
    arg: &BuildArg,
    module: Option<&str>,
) -> Result<TagCopy<'s>, ToolError> {
    let (build, tag) = self::tag(session, text, arg)?;
    let copy = query::choose_copy(build, tag, module_filter(module).as_ref()).ok_or_else(|| {
        ToolError::new(format!(
            "{} has no copy in a module matching '{}'",
            build.display_name(tag),
            module.unwrap_or_default()
        ))
        .hint("list_entries without module= lists its modules")
    })?;
    let other_copies_hint = (tag.copies.len() > 1).then(|| {
        let modules: Vec<&str> = tag.copies.iter().map(|&c| build.module(c).rel_path.as_str()).collect();
        present::other_copies_hint(&modules, &build.module(copy).rel_path)
    });
    Ok(TagCopy { build, tag, copy, other_copies_hint })
}

pub fn name_matcher(query: &str, mode: Mode) -> Result<NameMatcher, ToolError> {
    NameMatcher::new(query, mode).map_err(|e| ToolError::new(e).hint("mode=regex uses Rust regex syntax"))
}

/// A class code from `class=` (code or name), also for classes with no
/// indexed tags but a definition: references often point to those.
pub fn class_code(build: &Build, query: &str) -> Result<FourCc, ToolError> {
    match build.lookup_class(query) {
        ClassLookup::Found(info) => Ok(info.code),
        ClassLookup::DefinitionOnly(d) => FourCc::from_code(&d.file_code)
            .ok_or_else(|| ToolError::new(format!("class '{query}' has no usable code ({})", d.file_code))),
        ClassLookup::Unknown => {
            Err(ToolError::new(format!("no class '{query}'")).hint("list_classes lists the classes and their codes"))
        }
    }
}

/// `query_field`'s `filter`: `<op> <value>` (PRD §6.6).
pub fn field_filter(text: &str) -> Result<FieldFilter, ToolError> {
    let text = text.trim();
    let bad = || {
        ToolError::new(format!("can't read the filter '{text}'"))
            .hint("operators: == != > >= < <= contains, then a value (e.g. '>= 30')")
    };
    let (op, rest) =
        Op::ALL.iter().find_map(|&(name, op)| text.strip_prefix(name).map(|rest| (op, rest.trim()))).ok_or_else(bad)?;
    if rest.is_empty() {
        return Err(bad());
    }
    Ok(FieldFilter { op, value: rest.to_owned() })
}
pub fn module_filter(text: Option<&str>) -> Option<ModuleFilter> {
    text.map(ModuleFilter::new)
}

/// A tool as MCP lists it (PRD §9a): its name, its description (the
/// parameter struct's doc comment, also the CLI help) and its input schema.
pub struct ToolSpec {
    pub name: &'static str,
    pub description: String,
    pub input_schema: Map<String, Value>,
}

fn schema<T: schemars::JsonSchema>() -> Map<String, Value> {
    let Value::Object(o) = schemars::schema_for!(T).to_value() else { unreachable!("a struct's schema is an object") };
    o
}

pub(super) fn spec<T: super::Tool + schemars::JsonSchema>() -> ToolSpec {
    let mut input_schema = schema::<T>();
    // The description goes to the tool; the rest is the arguments' schema.
    let description = input_schema.remove("description").and_then(|d| d.as_str().map(str::to_owned));
    for key in ["$schema", "title"] {
        input_schema.remove(key);
    }
    ToolSpec { name: T::NAME, description: description.unwrap_or_default(), input_schema }
}

/// Parses `args` into tool `name`'s parameters. Unknown keys are refused
/// (serde would drop them, and a misspelt `path=` would read the whole tag).
pub(super) fn from_json<T: serde::de::DeserializeOwned + schemars::JsonSchema>(
    name: &str,
    args: Value,
) -> Result<T, ToolError> {
    let schema = schema::<T>();
    let known: Vec<&str> = schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|p| p.keys().map(String::as_str).collect())
        .unwrap_or_default();
    let params = || format!("parameters: {}", known.join(", "));
    if let Value::Object(o) = &args
        && let Some(k) = o.keys().find(|k| !known.contains(&k.as_str()))
    {
        return Err(ToolError::new(format!("{name} has no parameter '{k}'")).hint(params()));
    }
    serde_json::from_value(args).map_err(|e| ToolError::new(format!("{name}: {e}")).hint(params()))
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paging() {
        let v = [1, 2, 3, 4, 5];
        let page = |offset, limit| PageArgs { offset, limit };
        assert_eq!(page(0, Some(2)).page(&v, 100), &[1, 2]);
        assert_eq!(page(4, None).page(&v, 100), &[5]);
        assert_eq!(page(1, None).page(&v, 2), &[2, 3]);
        assert!(page(9, Some(10)).page(&v, 100).is_empty());
    }

    /// `query_field`'s filter (PRD §6.6): `<op> <value>`, spaces optional after the op.
    #[test]
    fn field_filters_are_an_operator_and_a_value() {
        use crate::field_query::{FieldFilter, Op};
        let f = |op, value: &str| FieldFilter { op, value: value.into() };
        assert_eq!(field_filter("== 36").unwrap(), f(Op::Eq, "36"));
        assert_eq!(field_filter(">=2.5").unwrap(), f(Op::Ge, "2.5"));
        assert_eq!(field_filter("> 1").unwrap(), f(Op::Gt, "1"));
        assert_eq!(field_filter("!= none").unwrap(), f(Op::Ne, "none"));
        assert_eq!(field_filter("<= -1").unwrap(), f(Op::Le, "-1"));
        assert_eq!(field_filter("contains  plasma rifle ").unwrap(), f(Op::Contains, "plasma rifle"));
        let err = field_filter("~ 3").unwrap_err();
        assert_eq!(err.hint.as_deref(), Some("operators: == != > >= < <= contains, then a value (e.g. '>= 30')"));
        assert!(field_filter("==").is_err(), "an operator needs a value");
    }
}
