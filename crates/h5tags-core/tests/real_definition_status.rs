//! `definition_status` on a real build, as the agent reads it: the overview's
//! first rows, then each class asked for. For checking the definition check
//! when the programs themselves can't run (D54). Ignored by default.
//!
//! ```text
//! H5_CONFIG=<config> H5_BUILD=<build> H5_CLASSES=sngl,wgtz,cnmp \
//!   cargo test -p h5tags-core --test real_definition_status -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use h5tags_core::output::Format;
use h5tags_core::tools::args::{BuildArg, PageArgs};
use h5tags_core::tools::{DefinitionStatus, Tool};
use h5tags_core::{Config, Session};

#[test]
#[ignore]
fn definition_status_as_the_agent_reads_it() {
    let config = PathBuf::from(std::env::var("H5_CONFIG").expect("set H5_CONFIG"));
    let build = std::env::var("H5_BUILD").ok();
    let classes = std::env::var("H5_CLASSES").unwrap_or_default();
    let mut s = Session::open(Config::load(&config).unwrap()).unwrap();
    let run = |s: &mut Session, class: Option<&str>, limit: Option<usize>| {
        let t = DefinitionStatus {
            class: class.map(Into::into),
            build: BuildArg { build: build.clone() },
            page: PageArgs { offset: 0, limit },
        };
        t.run(s).map(|o| o.render(Format::Text)).unwrap_or_else(|e| e.render(Format::Text))
    };
    println!("{}", run(&mut s, None, Some(12)));
    for class in classes.split(',').filter(|c| !c.is_empty()) {
        println!("== {class}\n{}", run(&mut s, Some(class), None));
    }
}
