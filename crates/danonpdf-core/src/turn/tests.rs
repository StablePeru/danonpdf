//! Tests for turns. No test here opens a file: a turn is a small value,
//! and reading one from text needs no document.

use super::*;

/// Reads `text` as a turn.
fn parse(text: &str) -> Result<Turn, TurnSyntaxError> {
    text.parse()
}

// Reading a turn from text

#[test]
fn the_four_turns_are_written_as_their_degrees() {
    assert_eq!(parse("0"), Ok(Turn::Zero));
    assert_eq!(parse("90"), Ok(Turn::Quarter));
    assert_eq!(parse("180"), Ok(Turn::Half));
    assert_eq!(parse("270"), Ok(Turn::ThreeQuarters));
}

#[test]
fn spaces_around_the_degrees_are_ignored() {
    assert_eq!(parse(" 90 "), Ok(Turn::Quarter));
}

#[test]
fn any_other_text_is_not_a_turn_and_is_named_in_the_error() {
    // Each of these means something to a person (a turn to the left, a
    // full turn, half a quarter), and none is one of the four spellings.
    for text in [
        "", " ", "45", "360", "-90", "+90", "90.0", "090", "left", "90,180",
    ] {
        assert_eq!(
            parse(text),
            Err(TurnSyntaxError {
                text: text.to_owned()
            }),
            "{text:?}"
        );
    }
}

#[test]
fn the_error_quotes_the_text_and_lists_what_is_allowed() {
    assert_eq!(
        parse("45").unwrap_err().to_string(),
        "\"45\" is not a turn: write 0, 90, 180 or 270 (degrees to the right)"
    );
}

// What a turn is worth

#[test]
fn a_turn_is_worth_its_degrees() {
    assert_eq!(Turn::Zero.degrees(), 0);
    assert_eq!(Turn::Quarter.degrees(), 90);
    assert_eq!(Turn::Half.degrees(), 180);
    assert_eq!(Turn::ThreeQuarters.degrees(), 270);
}

#[test]
fn text_and_degrees_agree() {
    // What `degrees` gives is what `parse` reads back.
    for turn in Turn::ALL {
        assert_eq!(parse(&turn.degrees().to_string()), Ok(turn));
    }
}

// One turn after another

#[test]
fn turns_add_up() {
    assert_eq!(Turn::Quarter.then(Turn::Quarter), Turn::Half);
    assert_eq!(Turn::Quarter.then(Turn::Half), Turn::ThreeQuarters);
    assert_eq!(Turn::Half.then(Turn::Quarter), Turn::ThreeQuarters);
}

#[test]
fn three_quarter_turns_and_one_more_is_no_turn() {
    assert_eq!(Turn::ThreeQuarters.then(Turn::Quarter), Turn::Zero);
}

#[test]
fn a_full_turn_starts_again_from_zero() {
    assert_eq!(Turn::Half.then(Turn::Half), Turn::Zero);
    assert_eq!(Turn::ThreeQuarters.then(Turn::Half), Turn::Quarter);
    assert_eq!(Turn::ThreeQuarters.then(Turn::ThreeQuarters), Turn::Half);
}

#[test]
fn no_turn_changes_nothing() {
    for turn in Turn::ALL {
        assert_eq!(turn.then(Turn::Zero), turn);
        assert_eq!(Turn::Zero.then(turn), turn);
    }
}

// The turn behind a `/Rotate`

#[test]
fn a_rotation_of_whole_quarters_is_a_turn() {
    assert_eq!(Turn::from_rotation(0), Some(Turn::Zero));
    assert_eq!(Turn::from_rotation(90), Some(Turn::Quarter));
    assert_eq!(Turn::from_rotation(180), Some(Turn::Half));
    assert_eq!(Turn::from_rotation(270), Some(Turn::ThreeQuarters));
}

#[test]
fn a_rotation_outside_0_to_270_is_brought_back_into_it() {
    // -90 is a quarter turn to the left, which is three to the right.
    assert_eq!(Turn::from_rotation(-90), Some(Turn::ThreeQuarters));
    assert_eq!(Turn::from_rotation(-180), Some(Turn::Half));
    assert_eq!(Turn::from_rotation(360), Some(Turn::Zero));
    assert_eq!(Turn::from_rotation(450), Some(Turn::Quarter));
    // The lowest number there is must not overflow on the way.
    assert_eq!(Turn::from_rotation(i64::MIN), None);
    assert_eq!(Turn::from_rotation(i64::MAX), None);
}

#[test]
fn a_rotation_that_is_not_a_multiple_of_90_has_no_turn() {
    for degrees in [1, 45, 89, 91, -45, 359] {
        assert_eq!(Turn::from_rotation(degrees), None, "{degrees}");
    }
}
