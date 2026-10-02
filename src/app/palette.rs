//! Quick open and search overlays.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use iced::Task;
use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, StreamExt};
use iced::widget::operation;

use super::editing::focus_editor;
use super::{Message, State, blocking, delayed};
use crate::search::{self, SearchHit, SearchSummary};
use crate::workspace;

pub const PALETTE_INPUT_ID: &str = "palette-input";
const QUICK_OPEN_LIMIT: usize = 50;
const SEARCH_DELAY: Duration = Duration::from_millis(200);

pub enum Palette {
    QuickOpen {
        query: String,
        results: Vec<String>,
        selected: usize,
    },
    Search(SearchPalette),
}

#[derive(Default)]
pub struct SearchPalette {
    pub query: String,
    pub regex: bool,
    pub hits: Vec<SearchHit>,
    pub summary: Option<SearchSummary>,
    pub error: Option<String>,
    pub selected: usize,
    pub generation: u64,
    pub cancel: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
pub enum SearchEvent {
    Batch(Vec<SearchHit>),
    Done(Result<SearchSummary, String>),
}

pub fn build_index(root: PathBuf) -> Task<Message> {
    Task::perform(
        blocking(move || workspace::walk_notes(&root)),
        Message::IndexBuilt,
    )
}

fn run_search(
    root: PathBuf,
    query: String,
    regex: bool,
    generation: u64,
    cancel: Arc<AtomicBool>,
) -> Task<Message> {
    let stream = iced::stream::channel(16, async move |mut output: mpsc::Sender<SearchEvent>| {
        let (tx, mut rx) = mpsc::unbounded();
        let worker = tokio::task::spawn_blocking(move || {
            search::search(&root, &query, regex, &cancel, |batch| {
                let _ = tx.unbounded_send(batch);
            })
        });
        while let Some(batch) = rx.next().await {
            let _ = output.send(SearchEvent::Batch(batch)).await;
        }
        let result = worker.await.unwrap_or_else(|err| Err(err.to_string()));
        let _ = output.send(SearchEvent::Done(result)).await;
    });
    Task::run(stream, move |event| Message::Search(generation, event))
}

impl State {
    pub(super) fn index_built(&mut self, notes: Vec<PathBuf>) -> Task<Message> {
        self.quick_open.set_notes(notes);
        if let Some(Palette::QuickOpen { query, results, .. }) = &mut self.palette {
            *results = self.quick_open.matches(query, QUICK_OPEN_LIMIT);
        }
        Task::none()
    }

    pub(super) fn open_quick_open(&mut self) -> Task<Message> {
        if matches!(self.palette, Some(Palette::QuickOpen { .. })) {
            return self.close_palette();
        }
        let results = self.quick_open.matches("", QUICK_OPEN_LIMIT);
        self.palette = Some(Palette::QuickOpen {
            query: String::new(),
            results,
            selected: 0,
        });
        operation::focus(PALETTE_INPUT_ID)
    }

    pub(super) fn open_search(&mut self) -> Task<Message> {
        if matches!(self.palette, Some(Palette::Search(_))) {
            return self.close_palette();
        }
        self.palette = Some(Palette::Search(SearchPalette::default()));
        operation::focus(PALETTE_INPUT_ID)
    }

    pub(super) fn palette_query(&mut self, text: String) -> Task<Message> {
        match &mut self.palette {
            Some(Palette::QuickOpen {
                query,
                results,
                selected,
            }) => {
                *results = self.quick_open.matches(&text, QUICK_OPEN_LIMIT);
                *query = text;
                *selected = 0;
                Task::none()
            }
            Some(Palette::Search(search)) => {
                search.query = text;
                self.restart_search()
            }
            None => Task::none(),
        }
    }

    /// Cancels any running search and schedules a new one.
    fn restart_search(&mut self) -> Task<Message> {
        self.search_generation += 1;
        let generation = self.search_generation;
        let Some(Palette::Search(search)) = &mut self.palette else {
            return Task::none();
        };
        search.cancel.store(true, Ordering::Relaxed);
        search.generation = generation;
        search.hits.clear();
        search.summary = None;
        search.error = None;
        search.selected = 0;
        if search.query.trim().is_empty() {
            return Task::none();
        }
        delayed(SEARCH_DELAY, Message::SearchDue(generation))
    }

    pub(super) fn search_toggle_regex(&mut self, regex: bool) -> Task<Message> {
        if let Some(Palette::Search(search)) = &mut self.palette {
            search.regex = regex;
        }
        self.restart_search()
    }

    pub(super) fn search_due(&mut self, generation: u64) -> Task<Message> {
        let root = self.root.clone();
        match &mut self.palette {
            Some(Palette::Search(search)) if search.generation == generation => {
                search.cancel = Arc::new(AtomicBool::new(false));
                let query = search.query.clone();
                run_search(root, query, search.regex, generation, search.cancel.clone())
            }
            _ => Task::none(),
        }
    }

    pub(super) fn search_event(&mut self, generation: u64, event: SearchEvent) -> Task<Message> {
        let Some(Palette::Search(search)) = &mut self.palette else {
            return Task::none();
        };
        if search.generation != generation {
            return Task::none();
        }
        match event {
            SearchEvent::Batch(hits) => search.hits.extend(hits),
            SearchEvent::Done(Ok(summary)) => search.summary = Some(summary),
            SearchEvent::Done(Err(error)) => search.error = Some(error),
        }
        Task::none()
    }

    /// Moves the highlighted result, clamped to the list.
    pub(super) fn palette_move(&mut self, delta: isize) {
        let (selected, len) = match &mut self.palette {
            Some(Palette::QuickOpen {
                selected, results, ..
            }) => (selected, results.len()),
            Some(Palette::Search(search)) => (&mut search.selected, search.hits.len()),
            None => return,
        };
        *selected = selected
            .saturating_add_signed(delta)
            .min(len.saturating_sub(1));
    }

    pub(super) fn palette_submit(&mut self) -> Task<Message> {
        match &self.palette {
            Some(Palette::QuickOpen { selected, .. }) => self.palette_pick(*selected),
            Some(Palette::Search(search)) => self.palette_pick(search.selected),
            None => Task::none(),
        }
    }

    pub(super) fn palette_pick(&mut self, index: usize) -> Task<Message> {
        match self.palette.take() {
            Some(Palette::QuickOpen { results, .. }) => match results.get(index) {
                Some(relative) => {
                    let path = self.quick_open.absolute(relative);
                    self.open(path)
                }
                None => Task::none(),
            },
            Some(Palette::Search(search)) => {
                search.cancel.store(true, Ordering::Relaxed);
                match search.hits.get(index) {
                    Some(hit) => self.open_at(hit.path.clone(), hit.line.saturating_sub(1)),
                    None => Task::none(),
                }
            }
            None => Task::none(),
        }
    }

    pub(super) fn close_palette(&mut self) -> Task<Message> {
        if let Some(Palette::Search(search)) = &self.palette {
            search.cancel.store(true, Ordering::Relaxed);
        }
        self.palette = None;
        focus_editor()
    }
}
