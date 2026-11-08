use h5_defs::definition::Definition;
use h5_meta::{Body, read_tag};
use h5_module::tag::TagHeader;
use h5_module::testutil::{TestTag, build_tag, sample_tag};

use super::*;
use crate::testdeploy::TestDeploy;

/// An enum (latch = 1), flags (second = bit 1) and an int (36).
const DEFINITION: &str = r#"<plugin>
	<_field_short_enum name="mode"><item Name="normal" /><item Name="latch" /></_field_short_enum>
	<_field_long_flags name="flags"><item Name="first" /><item Name="second" /></_field_long_flags>
	<_field_long_integer name="rounds" />
</plugin>"#;

fn tag() -> TestTag {
    let mut main = Vec::new();
    main.extend(1i16.to_le_bytes());
    main.extend(0b10u32.to_le_bytes());
    main.extend(36i32.to_le_bytes());
    let mut t = sample_tag();
    t.deps.clear();
    t.tag_refs.clear();
    t.data_refs.clear();
    t.blocks = vec![main];
    t.structs = vec![(0, 0, -1, 0, [1; 16], 0)];
    t
}

fn with_root(f: impl FnOnce(&Body<'_>)) {
    let def = Definition::parse(DEFINITION).unwrap();
    let (h, data) = build_tag(&tag());
    let r = read_tag(&def, &TagHeader::parse(&h).unwrap(), &data);
    f(&r.root);
}

fn filter(op: Op, value: &str) -> FieldFilter {
    FieldFilter::Compare { op, value: value.into() }
}

/// The options of the definition's enum (`mode`) and flags (`flags`).
fn options(field: usize) -> Vec<h5_defs::name::Name> {
    Definition::parse(DEFINITION).unwrap().fields[field].options.clone()
}

const KINDS: [ValueKind; 7] = [
    ValueKind::Number,
    ValueKind::Enum,
    ValueKind::Flags,
    ValueKind::Text,
    ValueKind::Multi,
    ValueKind::Data,
    ValueKind::Padding,
];

#[test]
fn filters_fit_the_kind_of_value() {
    let (mode, flags) = (options(0), options(1));
    let ok = |kind, f: &FieldFilter| {
        let options = if kind == ValueKind::Flags { &flags } else { &mode };
        check_filter("f", kind, options, f).is_ok()
    };
    assert!(ok(ValueKind::Number, &filter(Op::Ge, "2.5")));
    assert!(!ok(ValueKind::Number, &filter(Op::Contains, "2")));
    assert_eq!(
        check_filter("f", ValueKind::Number, &[], &filter(Op::Eq, "many")),
        Err(QueryError::NotANumber { value: "many".into() })
    );
    assert!(
        ok(ValueKind::Enum, &filter(Op::Eq, "LATCH"))
            && ok(ValueKind::Enum, &filter(Op::Gt, "0"))
            && !ok(ValueKind::Enum, &filter(Op::Gt, "latch"))
    );
    assert!(ok(ValueKind::Flags, &filter(Op::Contains, "first")) && !ok(ValueKind::Flags, &filter(Op::Eq, "2")));
    assert!(ok(ValueKind::Text, &filter(Op::Contains, "bullet")) && !ok(ValueKind::Text, &filter(Op::Gt, "a")));
    let err = check_filter("error angle", ValueKind::Multi, &[], &filter(Op::Eq, "1")).unwrap_err();
    assert_eq!(
        err.to_string(),
        "error angle is several numbers (bounds, a point, a vector or a color): a comparison doesn't apply to it"
    );
    assert!(check_sort("error angle", ValueKind::Multi).is_err() && check_sort("rounds", ValueKind::Number).is_ok());
}

/// D69: `is set` / `is empty` fit every kind but enums (option 0 is a value,
/// not an empty one) and padding (no value at all).
#[test]
fn is_set_and_is_empty_fit_every_kind_with_a_value() {
    for kind in KINDS {
        let fits = check_filter("f", kind, &[], &FieldFilter::IsSet).is_ok();
        assert_eq!(fits, !matches!(kind, ValueKind::Enum | ValueKind::Padding), "{kind:?}");
        assert_eq!(check_filter("f", kind, &[], &FieldFilter::IsEmpty).is_ok(), fits, "{kind:?}");
    }
    assert_eq!(
        check_filter("mode", ValueKind::Enum, &[], &FieldFilter::IsSet).unwrap_err().to_string(),
        "mode is an enum: is set or is empty doesn't apply to it"
    );
}

/// D69: a label the definition doesn't have is an error, not a filter that
/// matches every tag (`!= lacth`) or none (`contains secnd`).
#[test]
fn enum_and_flag_labels_must_be_options() {
    let (mode, flags) = (options(0), options(1));
    assert_eq!(
        check_filter("mode", ValueKind::Enum, &mode, &filter(Op::Ne, "lacth")),
        Err(QueryError::NotAnOption {
            field: "mode".into(),
            value: "lacth".into(),
            options: vec!["normal".into(), "latch".into()]
        })
    );
    assert!(check_filter("mode", ValueKind::Enum, &mode, &filter(Op::Eq, "7")).is_ok(), "a number needs no label");
    let err = check_filter("flags", ValueKind::Flags, &flags, &filter(Op::Contains, "secnd")).unwrap_err();
    assert_eq!(err.to_string(), "'secnd' isn't an option of flags");
}

/// The hint's operator list (`operators`) is the list `check_filter` accepts.
#[test]
fn each_kind_lists_the_filters_it_takes() {
    let listed = |kind: ValueKind, word: &str| kind.operators().split([' ', ',']).any(|w| w == word);
    for kind in KINDS {
        for (name, op) in Op::ALL {
            let fits = !matches!(check_filter("f", kind, &[], &filter(op, "1")), Err(QueryError::Unsupported { .. }));
            assert_eq!(fits, listed(kind, name), "{kind:?} {name}");
        }
        let fits = check_filter("f", kind, &[], &FieldFilter::IsSet).is_ok();
        assert_eq!(fits, listed(kind, "set") && listed(kind, "empty"), "{kind:?} is set");
    }
}

/// D69: `is empty` is what the view writes as `(none)`, `(empty)` or `(none
/// set)`, an empty string, or zero.
#[test]
fn values_that_hold_nothing() {
    use h5_meta::{DataRef, ResourceRef, StringIdValue};
    let empty = [
        Value::Int(0),
        Value::Uint(0),
        Value::Real(-0.0),
        Value::Ints(vec![0, 0]),
        Value::Reals(vec![0.0, 0.0, 0.0]),
        Value::Flags(0),
        Value::BlockIndex(-1),
        Value::Color(0),
        Value::String(String::new()),
        Value::StringId(StringIdValue { hash: 0, text: None }),
        Value::Tag(FourCc::NONE),
        Value::Data(DataRef { size: 0, target: None, fingerprint: None }),
        Value::Resource(ResourceRef { index: None }),
    ];
    for v in &empty {
        assert!(holds_nothing(v), "{v:?}");
    }
    let set = [
        Value::Int(-1),
        Value::Real(0.5),
        Value::Reals(vec![0.0, 1.0]),
        Value::Flags(0b100),
        Value::BlockIndex(0),
        Value::String("a".into()),
        Value::StringId(StringIdValue { hash: 7, text: None }),
        Value::Tag(FourCc::from_code("weap").unwrap()),
        Value::Data(DataRef { size: 4, target: Some(1), fingerprint: None }),
        Value::Resource(ResourceRef { index: Some(0) }),
    ];
    for v in &set {
        assert!(!holds_nothing(v), "{v:?}");
    }
}

#[test]
fn values_pass_filters_by_number_label_and_flag() {
    let d = TestDeploy::new("field-query-pass");
    let b = &d.session.builds()[0];
    with_root(|root| {
        let [mode, flags, rounds] = [&root.children[0], &root.children[1], &root.children[2]];
        assert!(passes(b, rounds, &filter(Op::Eq, "36")) && passes(b, rounds, &filter(Op::Gt, "35.5")));
        assert!(!passes(b, rounds, &filter(Op::Lt, "36")));
        assert!(passes(b, mode, &filter(Op::Eq, "LATCH")) && passes(b, mode, &filter(Op::Eq, "1")));
        assert!(passes(b, mode, &filter(Op::Ne, "normal")) && !passes(b, mode, &filter(Op::Eq, "normal")));
        assert!(passes(b, flags, &filter(Op::Contains, "second")) && !passes(b, flags, &filter(Op::Contains, "first")));
        assert!(passes(b, rounds, &FieldFilter::IsSet) && !passes(b, rounds, &FieldFilter::IsEmpty));
        assert!(passes(b, flags, &FieldFilter::IsSet));
        assert_eq!(SortKey::of(b, mode), Some(SortKey::Text("latch".into())));
        assert_eq!(SortKey::of(b, rounds), Some(SortKey::Number(36.0)));
    });
}

/// D75: an `also` path's `[*]` takes the row's entry, at any depth; a `[*]`
/// the main path doesn't have (or has as an index) is refused.
#[test]
fn also_paths_take_the_rows_entries() {
    let xml = r#"<plugin>
	<_field_block_64 name="barrels">
		<_field_block_64 name="shots"><_field_real name="x" /></_field_block_64>
		<_field_real name="heat" />
	</_field_block_64>
	<_field_long_integer name="count" />
</plugin>"#;
    let def = Definition::parse(xml).unwrap();
    let p = |t: &str| h5_meta::parse_path(&def.fields, t).unwrap();
    let path = p("barrels[*].shots[*].x");
    let row = p("barrels[1].shots[2].x");
    for (also, at) in
        [("barrels[*].heat", "barrels[1].heat"), ("barrels[*].shots[*].x", "barrels[1].shots[2].x"), ("count", "count")]
    {
        assert_eq!(check_also(&p(also), &path, &def.fields), Ok(()), "{also}");
        assert_eq!(also_at(&p(also), &row).display(&def.fields), at);
    }
    assert_eq!(
        check_also(&p("barrels[*].heat"), &p("barrels[0].shots[*].x"), &def.fields),
        Err(QueryError::AlsoNotAligned {
            also: "barrels[*].heat".into(),
            path: "barrels[0].shots[*].x".into(),
            example: "barrels[0]".into()
        })
    );
}
