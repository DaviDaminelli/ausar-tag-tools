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
    FieldFilter { op, value: value.into() }
}

#[test]
fn filters_fit_the_kind_of_value() {
    let ok = |kind, op, value| check_filter("f", kind, &filter(op, value)).is_ok();
    assert!(ok(ValueKind::Number, Op::Ge, "2.5"));
    assert!(!ok(ValueKind::Number, Op::Contains, "2"));
    assert_eq!(
        check_filter("f", ValueKind::Number, &filter(Op::Eq, "many")),
        Err(QueryError::NotANumber { value: "many".into() })
    );
    assert!(
        ok(ValueKind::Enum, Op::Eq, "latch")
            && ok(ValueKind::Enum, Op::Gt, "0")
            && !ok(ValueKind::Enum, Op::Gt, "latch")
    );
    assert!(ok(ValueKind::Flags, Op::Contains, "first") && !ok(ValueKind::Flags, Op::Eq, "2"));
    assert!(ok(ValueKind::Text, Op::Contains, "bullet") && !ok(ValueKind::Text, Op::Gt, "a"));
    let err = check_filter("error angle", ValueKind::Multi, &filter(Op::Eq, "1")).unwrap_err();
    assert_eq!(
        err.to_string(),
        "error angle is several numbers (bounds, a point, a vector or a color): a filter doesn't apply to it"
    );
    assert!(check_sort("error angle", ValueKind::Multi).is_err() && check_sort("rounds", ValueKind::Number).is_ok());
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
        assert_eq!(SortKey::of(b, mode), Some(SortKey::Text("latch".into())));
        assert_eq!(SortKey::of(b, rounds), Some(SortKey::Number(36.0)));
    });
}
