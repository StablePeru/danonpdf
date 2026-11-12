//! Which pages to take, written as text such as `1-3,7,10-`.
//!
//! The text is a list of items separated by commas. Each item is one of:
//!
//! - `N`: page N;
//! - `A-B`: pages A to B, both included, with A not above B;
//! - `A-`: page A and every page after it, up to the last one.
//!
//! Pages are counted from 1. The order of the items is kept: `3,1` means
//! page 3 and then page 1. Spaces around an item are ignored.
//!
//! Understanding the text takes two steps, kept apart on purpose:
//!
//! 1. **Parse**: turn the text into a [`PageSelection`]. This needs no
//!    document, so a front end can refuse a mistyped selection before it
//!    opens any file. What can go wrong is a [`PageSyntaxError`].
//! 2. **Resolve**: turn the selection into page numbers for a document of a
//!    known length ([`PageSelection::resolve`]). Only now is it known where
//!    `A-` ends, and whether every page asked for exists. What can go wrong
//!    is [`PdfError::PageOutOfRange`].
//!
//! The two steps fail for different reasons, and a caller treats them
//! differently: the first is a mistake in what was typed, the second is a
//! valid request that this document cannot satisfy.

use std::error::Error;
use std::fmt;
use std::ops::RangeInclusive;
use std::str::FromStr;

use crate::PdfError;

/// The pages chosen by a text such as `1-3,7,10-`, not yet tied to any
/// document.
///
/// The text is a list of items separated by commas: `N` is one page, `A-B`
/// the pages from A to B, and `A-` page A and every page after it. Pages
/// are counted from 1, and the order of the items is kept. The top of
/// `pages.rs` has the rules in full.
///
/// Build one with `text.parse()`, which works because of the [`FromStr`]
/// implementation below.
///
/// A value of this type has already passed every check that needs no
/// document: every item is readable, no page is 0, no range runs backwards
/// and no page is listed twice. Code that receives one does not have to
/// check any of that again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageSelection {
    /// The items, in the order they were written. Never empty.
    ranges: Vec<PageRange>,
}

/// One item of a selection.
///
/// An enum and not, say, a pair of numbers with a flag: each kind of item
/// carries exactly the numbers it has, and no more. `ToTheEnd` has no last
/// page to store, so there is no field that would hold a made-up value
/// until the document is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageRange {
    /// `N`: one page.
    Single(usize),
    /// `A-B`: from `first` to `last`, both included. `first` is never above
    /// `last`: parsing refuses the item otherwise.
    Closed { first: usize, last: usize },
    /// `A-`: from `first` to the last page of whatever document this is
    /// used on.
    ToTheEnd { first: usize },
}

impl PageRange {
    /// Reads one item, already without the spaces around it. `position` is
    /// its place in the list, counted from 1, for the error message.
    fn parse(item: &str, position: usize) -> Result<Self, PageSyntaxError> {
        if item.is_empty() {
            return Err(PageSyntaxError::EmptyItem { position });
        }

        // `split_once` cuts the item at its first "-" and gives the two
        // sides, or `None` when there is no "-" at all. The three arms are
        // the three kinds of item. The second arm is tried before the third
        // and only takes an empty right side (`""` is a pattern too: it
        // matches that exact text), so the third arm gets every other pair.
        match item.split_once('-') {
            None => Ok(Self::Single(page_number(item, item)?)),
            Some((first, "")) => Ok(Self::ToTheEnd {
                first: page_number(first, item)?,
            }),
            Some((first, last)) => {
                let first = page_number(first, item)?;
                let last = page_number(last, item)?;
                if first > last {
                    return Err(PageSyntaxError::BackwardsRange { first, last });
                }
                Ok(Self::Closed { first, last })
            }
        }
    }

    /// The pages this item stands for in a document whose last page is
    /// `last_page`, as a range of page numbers.
    ///
    /// Every arm gives the same type, `RangeInclusive<usize>` (written
    /// `a..=b`: from `a` to `b`, both included), so the caller does not
    /// care which kind of item it was. A single page is a range that starts
    /// and ends on itself.
    ///
    /// A range is only its two ends: building one costs the same for three
    /// pages as for three million. The pages come out one by one when
    /// something walks it.
    fn pages(self, last_page: usize) -> RangeInclusive<usize> {
        match self {
            Self::Single(page) => page..=page,
            Self::Closed { first, last } => first..=last,
            Self::ToTheEnd { first } => first..=last_page,
        }
    }

    /// The highest page number written in this item. If the document has
    /// that page, it has every page of the item.
    fn highest_named(self) -> usize {
        // `..` in a pattern skips the fields that are not needed.
        match self {
            Self::Single(page) => page,
            Self::Closed { last, .. } => last,
            Self::ToTheEnd { first } => first,
        }
    }
}

/// Reads `text`, one side of an item, as a page number. `item` is the whole
/// item, to be named if `text` is not one.
fn page_number(text: &str, item: &str) -> Result<usize, PageSyntaxError> {
    let not_a_number = || PageSyntaxError::NotANumber {
        item: item.to_owned(),
    };

    // Digits only. `parse` alone would also take "+3", and nobody writes a
    // page like that on purpose. `all` is true for an empty text (there is
    // no character that fails the test), hence the first check.
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(not_a_number());
    }
    match text.parse::<usize>() {
        // A pattern can be a plain value: this arm only takes zero.
        Ok(0) => Err(PageSyntaxError::PageZero),
        Ok(page) => Ok(page),
        // Only digits and still not a `usize`: the number is too big to be
        // one, and so far too big to be a page.
        Err(_) => Err(not_a_number()),
    }
}

/// The lowest page that two of the `ranges` share, if any.
///
/// The ranges are not listed page by page: `1-4000000000` would take a long
/// time and a lot of memory. Each range is looked at as its two ends
/// instead. Once they are sorted by where they start, two ranges that share
/// a page end up next to each other, with the second starting at or before
/// the end of the first. That start is the shared page.
fn first_repeated_page(ranges: &[PageRange]) -> Option<usize> {
    // No document is known here, so an open range is taken to run to the
    // highest number there is: it then meets everything after its start.
    let mut spans: Vec<RangeInclusive<usize>> =
        ranges.iter().map(|range| range.pages(usize::MAX)).collect();
    spans.sort_unstable_by_key(|span| *span.start());

    // `windows(2)` walks the list two neighbours at a time: (1st, 2nd),
    // (2nd, 3rd), and so on. `find` stops at the first pair that passes the
    // test, and `map` turns that pair into the page they share.
    spans
        .windows(2)
        .find(|pair| pair[1].start() <= pair[0].end())
        .map(|pair| *pair[1].start())
}

impl PageSelection {
    /// The page numbers this selection stands for in a document of
    /// `page_count` pages, in the order they were written.
    ///
    /// Fails with [`PdfError::PageOutOfRange`], and with nothing else, when
    /// an item names a page past the end. It names the first such item, by
    /// the highest page written in it.
    ///
    /// `&self`: the selection is only read, so the same one can be resolved
    /// against several documents.
    pub fn resolve(&self, page_count: usize) -> Result<Vec<usize>, PdfError> {
        // Checked before any page is listed, and from the ends of each
        // range only: `1-4000000000` on a 5-page document is refused at
        // once instead of filling the memory first.
        let past_the_end = self
            .ranges
            .iter()
            .map(|range| range.highest_named())
            .find(|&page| page > page_count);
        if let Some(page) = past_the_end {
            return Err(PdfError::PageOutOfRange {
                page,
                pages: page_count,
            });
        }

        // `map` would give a list of ranges; `flat_map` walks each range
        // and joins their pages into one list. No page is listed twice (the
        // parser saw to that), so the list is never longer than the
        // document.
        Ok(self
            .ranges
            .iter()
            .flat_map(|range| range.pages(page_count))
            .collect())
    }
}

/// Lets a selection be read with `text.parse::<PageSelection>()`, the same
/// way Rust reads a number from text. A library that takes values from text
/// (the command-line parser, for one) finds this implementation by itself.
impl FromStr for PageSelection {
    type Err = PageSyntaxError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        // Without this, "" would be reported as "item 1 is empty", which is
        // true but hides the simpler thing to say.
        if text.trim().is_empty() {
            return Err(PageSyntaxError::Empty);
        }

        // `enumerate` numbers the items from 0; people count from 1.
        // `collect` into a `Result` stops at the first item that fails and
        // returns its error, so the first mistake in the text is the one
        // reported.
        let ranges = text
            .split(',')
            .enumerate()
            .map(|(index, item)| PageRange::parse(item.trim(), index + 1))
            .collect::<Result<Vec<_>, _>>()?;

        if let Some(page) = first_repeated_page(&ranges) {
            return Err(PageSyntaxError::RepeatedPage { page });
        }
        Ok(Self { ranges })
    }
}

/// Why a text is not a page selection. None of these needs a document to
/// be found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageSyntaxError {
    /// There is no text at all, or only spaces.
    Empty,
    /// Nothing stands between two commas, or before the first or after the
    /// last. `position` counts the items from 1.
    EmptyItem { position: usize },
    /// The item is not `N`, `A-B` or `A-` with whole numbers. `item` is the
    /// text of that item.
    NotANumber { item: String },
    /// Page 0 was asked for; pages are counted from 1.
    PageZero,
    /// A range whose first page is above its last one, such as `5-3`.
    BackwardsRange { first: usize, last: usize },
    /// `page` is listed twice, on its own or inside a range.
    RepeatedPage { page: usize },
}

impl fmt::Display for PageSyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "no pages were given"),
            Self::EmptyItem { position } => {
                write!(f, "item {position} of the page list is empty")
            }
            // `{item:?}` prints the text in quotes, so stray spaces show.
            Self::NotANumber { item } => write!(
                f,
                "{item:?} is not a page (N) or a range of pages (A-B or A-)"
            ),
            Self::PageZero => write!(f, "pages are counted from 1: there is no page 0"),
            Self::BackwardsRange { first, last } => write!(
                f,
                "the range {first}-{last} runs backwards: \
                 a range goes from the lower page to the higher one"
            ),
            Self::RepeatedPage { page } => write!(f, "page {page} is listed more than once"),
        }
    }
}

// No `source()`: there is no lower-level error behind any of these.
impl Error for PageSyntaxError {}

#[cfg(test)]
mod tests;
