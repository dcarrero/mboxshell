use criterion::{criterion_group, criterion_main, Criterion};
use std::path::Path;

fn bench_parse_mbox(c: &mut Criterion) {
    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("simple.mbox");

    c.bench_function("parse_simple_mbox", |b| {
        b.iter(|| {
            let parser = mboxshell::parser::mbox::MboxParser::new(&fixture_path).unwrap();
            let mut count = 0u64;
            parser
                .parse(
                    &mut |_offset, _bytes| {
                        count += 1;
                        true
                    },
                    None,
                )
                .unwrap();
            count
        })
    });
}

fn bench_index_build(c: &mut Criterion) {
    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("simple.mbox");

    c.bench_function("build_index_simple", |b| {
        b.iter(|| mboxshell::index::builder::build_index(&fixture_path, true, None).unwrap())
    });
}

criterion_group!(benches, bench_parse_mbox, bench_index_build);
criterion_main!(benches);
