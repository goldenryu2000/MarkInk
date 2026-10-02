use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use criterion::{Criterion, criterion_group, criterion_main};
use iced::widget::markdown;
use iced::widget::text_editor::{Action, Edit};
use markink::document::{DocId, Document};
use markink::fsio::LoadedNote;
use markink::quick_open::QuickOpen;
use markink::undo::Change;

/// About 100 KB of mixed Markdown.
fn note_100kb() -> String {
    let block = "# Heading\n\nSome *text* with a [link](a.md) and `code`.\n\n- [ ] task\n- item\n\n```rust\nfn main() {}\n```\n\n";
    block.repeat(100_000 / block.len())
}

fn benches(c: &mut Criterion) {
    let text = note_100kb();
    let mut edited = text.clone();
    edited.insert(text.len() / 2, 'x');

    c.bench_function("diff 100KB", |b| {
        b.iter(|| Change::between(black_box(&text), black_box(&edited)))
    });

    c.bench_function("preview parse 100KB", |b| {
        b.iter(|| markdown::Content::parse(black_box(&text)))
    });

    let mut doc = Document::new(
        DocId(0),
        PathBuf::from("/n/a.md"),
        LoadedNote::from_text(&text),
    );
    c.bench_function("keystroke 100KB", |b| {
        b.iter(|| doc.apply(Action::Edit(Edit::Insert('x')), Instant::now()))
    });

    let mut index = QuickOpen::new(PathBuf::from("/n"));
    index.set_notes(
        (0..10_000)
            .map(|i| PathBuf::from(format!("/n/folder{}/note {i}.md", i % 50)))
            .collect(),
    );
    c.bench_function("quick open 10k", |b| {
        b.iter(|| index.matches(black_box("fol12 not 42"), 50))
    });
}

criterion_group!(core, benches);
criterion_main!(core);
