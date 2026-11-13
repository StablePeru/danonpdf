//! How far to turn a page: not at all, or one, two or three quarter turns
//! to the right.
//!
//! A PDF page says how it is to be shown with `/Rotate`, a whole number of
//! degrees clockwise that must be a multiple of 90 (PDF 1.7, section
//! 7.7.3.3, table 30).
//! There are therefore only four ways a page can be turned, and [`Turn`] is
//! an enum with those four and nothing else: a turn of 45 degrees cannot
//! even be written down.
//!
//! As with a page selection (`pages.rs`), reading the text a person typed
//! is kept apart from using the value: `"90".parse::<Turn>()` needs no
//! document, and what can go wrong there is a [`TurnSyntaxError`].

use std::error::Error;
use std::fmt;
use std::str::FromStr;

/// A turn to the right (clockwise), in quarters of a full turn.
///
/// It is relative: it says how much further a page is turned, not how the
/// page ends up. A page that already lies on its side and gets a
/// [`Turn::Quarter`] ends up upside down.
///
/// `Copy`: a turn is one small number inside, so it is passed around by
/// value like a number, and using it does not move it away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    /// No turn: the page stays as it is.
    Zero,
    /// A quarter turn to the right: 90 degrees.
    Quarter,
    /// Half a turn, upside down: 180 degrees.
    Half,
    /// Three quarter turns to the right: 270 degrees. The same as one
    /// quarter turn to the left.
    ThreeQuarters,
}

impl Turn {
    /// The four turns, for a test that wants to try every one. `cfg(test)`
    /// keeps it out of the library that ships, where nothing needs it.
    #[cfg(test)]
    const ALL: [Self; 4] = [Self::Zero, Self::Quarter, Self::Half, Self::ThreeQuarters];

    /// The turn in degrees clockwise: 0, 90, 180 or 270. This is the
    /// number a page keeps under `/Rotate`.
    pub fn degrees(self) -> i64 {
        match self {
            Self::Zero => 0,
            Self::Quarter => 90,
            Self::Half => 180,
            Self::ThreeQuarters => 270,
        }
    }

    /// This turn followed by `next`, as one turn. Four quarters are a full
    /// turn, which is no turn at all: three quarters and then one more
    /// give [`Turn::Zero`].
    ///
    /// `#[must_use]` makes the compiler warn when the result is thrown
    /// away: `turn.then(other);` on a line of its own changes nothing,
    /// because a `Turn` is a value and not something that is changed in
    /// place.
    #[must_use]
    pub fn then(self, next: Self) -> Self {
        // Both are 0, 90, 180 or 270, so the sum is a multiple of 90 and
        // `from_rotation` always finds its turn. `unwrap_or` still gives
        // the function something to return that is not a panic.
        Self::from_rotation(self.degrees() + next.degrees()).unwrap_or(Self::Zero)
    }

    /// The turn that a `/Rotate` of `degrees` stands for.
    ///
    /// Any whole number of quarter turns is taken, also one outside 0 to
    /// 270: `-90` is a quarter turn to the left, which ends where three to
    /// the right do, and `450` is a full turn and a quarter. A number that
    /// is not a multiple of 90 is not allowed by the PDF specification,
    /// and has no turn: `None`.
    pub(crate) fn from_rotation(degrees: i64) -> Option<Self> {
        // `%` keeps the sign of the number: `-90 % 360` is -90.
        // `rem_euclid` always lands in 0 to 359, so -90 becomes 270.
        match degrees.rem_euclid(360) {
            0 => Some(Self::Zero),
            90 => Some(Self::Quarter),
            180 => Some(Self::Half),
            270 => Some(Self::ThreeQuarters),
            _ => None,
        }
    }
}

/// Lets a turn be read with `text.parse::<Turn>()`, from the degrees
/// written as `0`, `90`, `180` or `270`. A library that takes values from
/// text (the command-line parser, for one) finds this by itself, as it
/// does for a page selection.
///
/// Only those four texts are taken. `-90` and `360` mean something to a
/// person, but a tool that accepts several spellings of one thing has to
/// document all of them; here there is one spelling per turn.
impl FromStr for Turn {
    type Err = TurnSyntaxError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        // A `match` can compare a text against literal texts. Spaces
        // around the number are ignored, as they are around the items of
        // a page selection.
        match text.trim() {
            "0" => Ok(Self::Zero),
            "90" => Ok(Self::Quarter),
            "180" => Ok(Self::Half),
            "270" => Ok(Self::ThreeQuarters),
            // `to_owned` copies the borrowed text into a `String` that
            // the error can keep after `text` is gone.
            _ => Err(TurnSyntaxError {
                text: text.to_owned(),
            }),
        }
    }
}

/// Why a text is not a turn. `text` is what was written.
///
/// A struct and not an enum: there is only one way for this to go wrong,
/// the text is none of the four that are allowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnSyntaxError {
    pub text: String,
}

impl fmt::Display for TurnSyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `{:?}` prints the text in quotes, so an empty one or stray
        // spaces show.
        write!(
            f,
            "{:?} is not a turn: write 0, 90, 180 or 270 (degrees to the right)",
            self.text
        )
    }
}

// No `source()`: there is no lower-level error behind this one.
impl Error for TurnSyntaxError {}

#[cfg(test)]
mod tests;
