//! Class inheritance (M9, decisions D97, D99, D100): the classes that inherit
//! a parent class (`object`, `item`, `unit`, `device`), and whether a field
//! of the parent is the same field in each. Domain only.
//!
//! A class inherits a parent when its definition has a class section named
//! after the parent (`OBJECT` for `object`); the parent's own definition is
//! the reference for its fields (format-notes "Custom fields").

use std::sync::Arc;

use h5_defs::definition::{Custom, Definition, Field, custom_role};
use h5_meta::{PathError, parse_path};
use h5_module::FourCc;

use crate::catalog::Build;
use crate::field_query::ClassPath;
use crate::stored::{DefinitionLoadError, load_class_definition};

/// Whether class section `label` (`SOUND SCENERY`) names class `name`
/// (`sound_scenery`): ignoring case, `_` and a space alike.
pub fn names_class(label: &str, name: &str) -> bool {
    let norm = |c: char| if c == '_' { ' ' } else { c.to_ascii_lowercase() };
    label.len() == name.len() && label.chars().map(norm).eq(name.chars().map(norm))
}

/// Whether `fields` (a definition's top level) has a class section named after `name`.
fn has_section(fields: &[Field], name: &str) -> bool {
    (0..fields.len()).any(|i| matches!(custom_role(fields, i), Some(Custom::ClassSection(s)) if names_class(&s, name)))
}

/// What [`inheriting_classes`] found.
#[derive(Debug, Default)]
pub struct Inheriting {
    /// Classes with tags, in code order.
    pub classes: Vec<FourCc>,
    /// Classes whose definition couldn't be loaded, so weren't looked at.
    pub unreadable: Vec<(FourCc, DefinitionLoadError)>,
}

/// The classes with tags (other than `parent`) whose definition has a class
/// section named after `parent`. None: `parent` is no parent.
pub fn inheriting_classes(b: &Build, parent: FourCc) -> Inheriting {
    let name = b.class_name(parent);
    let mut out = Inheriting::default();
    for (&code, info) in b.classes.iter().filter(|(c, i)| **c != parent && i.definition.is_some()) {
        match load_class_definition(b, code) {
            Ok(Some(def)) if has_section(&def.fields, &name) => out.classes.push(code),
            Ok(_) => {}
            Err(e) => out.unreadable.push((info.code, e)),
        }
    }
    out
}

#[derive(Debug)]
pub enum InheritError {
    /// The path isn't a field of the parent's definition; `classes`: those
    /// of the inheriting classes whose own definition has it.
    NotInParent {
        error: PathError,
        classes: Vec<FourCc>,
    },
    /// These classes don't have the parent's field there, or define it otherwise.
    Differs(Vec<FourCc>),
    Definition(FourCc, DefinitionLoadError),
}

/// `text` as a path of `parent` (its definition) in each of `classes`: one
/// [`ClassPath`] per class, each the parent's field defined the same way
/// (the field at the end of the path; a class reads it with its own
/// definition). Refused when the parent has no such field, or a class differs.
pub fn inherited_field(
    b: &Build,
    parent: &Definition,
    classes: &[FourCc],
    text: &str,
) -> Result<Vec<ClassPath>, InheritError> {
    let defs: Vec<(FourCc, Arc<Definition>)> = classes
        .iter()
        .filter_map(|&c| match load_class_definition(b, c) {
            Ok(Some(d)) => Some(Ok((c, d))),
            Ok(None) => None,
            Err(e) => Some(Err(InheritError::Definition(c, e))),
        })
        .collect::<Result<_, _>>()?;
    let reference = match parse_path(&parent.fields, text) {
        Ok(p) => p,
        Err(error) => {
            let classes = defs.iter().filter(|(_, d)| parse_path(&d.fields, text).is_ok()).map(|(c, _)| *c).collect();
            return Err(InheritError::NotInParent { error, classes });
        }
    };
    let field = reference.field(&parent.fields);
    let mut out = Vec::with_capacity(defs.len());
    let mut differ = Vec::new();
    for (class, def) in defs {
        match parse_path(&def.fields, text) {
            Ok(path) if path.field(&def.fields) == field => out.push(ClassPath { class, def, path }),
            _ => differ.push(class),
        }
    }
    if !differ.is_empty() {
        return Err(InheritError::Differs(differ));
    }
    Ok(out)
}

#[cfg(test)]
#[path = "inherit_tests.rs"]
mod tests;
