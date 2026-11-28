//! The Alchemist development card (it names the dice at the start of a turn) and the variable discard limit.

use crate::game::{Action, Building, Game, Phase, Settings};
use crate::log_event::LogEvent;
use crate::types::{ByDev, DevCard, Res};
use super::support::{all_connected, invariants, names, with, P0, P1};

/// Two players, the Alchemist switched on, and player 0 about to roll.
fn alchemist_game() -> Game {
    let mut g = Game::with_settings(names(2), with(|s| s.alchemist = true));
    g.phase = Phase::Roll;
    g.current = P0;
    g.rolled = false;
    g
}

/// A hex that makes cards when its number is rolled, and the number: where to put a building to see a roll pay.
fn producing_hex(g: &Game) -> (usize, u8) {
    let h = (0..g.board.hexes.len())
        .find(|&h| h != g.robber && g.board.hexes[h].number.is_some() && g.board.hexes[h].terrain.resource().is_some())
        .expect("a producing hex");
    (h, g.board.hexes[h].number.unwrap())
}

/// Two dice that show `total` (2 to 12).
fn dice_for(total: u8) -> (u8, u8) {
    let first = total.saturating_sub(6).max(1);
    (first, total - first)
}

// ----------------------------------------------------------------------
// The card in the deck
// ----------------------------------------------------------------------

#[test]
fn with_the_alchemist_on_a_25_card_deck_has_12_knights_and_2_alchemists() {
    let s = with(|s| s.alchemist = true);
    assert_eq!(s.dev_cards, 25);
    assert_eq!(s.deck_counts(), ByDev([12, 2, 2, 2, 5, 2]));
    let g = Game::with_settings(names(2), s);
    assert_eq!(g.deck.len(), 25);
    assert_eq!(g.deck.iter().filter(|&&c| c == DevCard::Knight).count(), 12);
    assert_eq!(g.deck.iter().filter(|&&c| c == DevCard::Alchemist).count(), 2);
    assert_eq!(g.deck.iter().filter(|&&c| c == DevCard::VictoryPoint).count(), 5);
}

#[test]
fn without_the_alchemist_the_deck_is_the_standard_one() {
    let g = Game::new(names(2));
    assert_eq!(g.deck.iter().filter(|&&c| c == DevCard::Alchemist).count(), 0);
    assert_eq!(g.deck.iter().filter(|&&c| c == DevCard::Knight).count(), 14);
}

#[test]
fn the_alchemist_deck_keeps_the_chosen_size_at_every_size() {
    for total in 0..=60u8 {
        let s = with(|s| {
            s.alchemist = true;
            s.dev_cards = total;
        });
        assert_eq!(s.deck_counts().0.iter().sum::<usize>(), total as usize, "deck of {total}");
        assert_eq!(Game::with_settings(names(2), s).deck.len(), total as usize);
    }
    assert_eq!(with(|s| { s.alchemist = true; s.dev_cards = 50 }).deck_counts(), ByDev([24, 4, 4, 4, 10, 4]));
}

#[test]
fn speed_mode_has_no_turns_so_no_alchemists_are_dealt() {
    let s = with(|s| {
        s.alchemist = true;
        s.speed = true;
    });
    assert_eq!(s.deck_counts(), ByDev([14, 2, 2, 2, 5, 0]));
    assert_eq!(Game::with_settings(names(2), s).deck.iter().filter(|&&c| c == DevCard::Alchemist).count(), 0);
}

#[test]
fn the_new_rules_are_named_in_the_log_only_when_they_are_on() {
    assert!(Settings::default().changes().is_empty());
    let changes = with(|s| {
        s.alchemist = true;
        s.variable_discard = true;
        s.discard_step_rolls = 8;
        s.discard_limit_cap = 11;
    })
    .changes();
    assert!(changes.contains(&"Alchemist card".to_string()), "{changes:?}");
    assert!(changes.iter().any(|c| c.contains("variable discard limit") && c.contains('8') && c.contains("11")), "{changes:?}");
}

// ----------------------------------------------------------------------
// Playing it
// ----------------------------------------------------------------------

#[test]
fn the_alchemist_names_the_dice_and_the_roll_follows() {
    let mut g = alchemist_game();
    let (hex, number) = producing_hex(&g);
    let corner = g.board.hexes[hex].vertices[0];
    g.buildings[corner] = Some(Building { owner: P0, city: false });
    g.players[P0].settlements_left -= 1;
    g.players[P0].dev[DevCard::Alchemist] = 1;
    let dice = dice_for(number);

    g.apply(P0, Action::PlayAlchemist { dice }).unwrap();

    assert_eq!(g.dice, Some(dice), "the dice show what was named");
    assert!(g.rolled);
    assert_eq!(g.phase, Phase::Main, "it is the roll, so the turn goes on to its main phase");
    assert!(g.players[P0].hand.total() >= 1, "the roll paid out");
    assert_eq!(g.players[P0].stats.dice_income, g.players[P0].hand.total());
    assert_eq!(g.players[P0].dev[DevCard::Alchemist], 0, "the card is used up");
    assert!(g.dev_played, "and it was the turn's development card");
    assert_eq!(g.roll_counts[usize::from(number) - 2], 1, "it counts as a roll");
    let at = g.log.iter().position(|e| *e == LogEvent::PlayedAlchemist { player: P0 }).expect("the play is logged");
    assert_eq!(g.log[at + 1], LogEvent::Rolled { player: P0, dice }, "and the roll right after it");
    invariants(&g);
}

#[test]
fn naming_a_seven_runs_the_seven_sequence() {
    // Nobody is over the limit: straight to the robber.
    let mut g = alchemist_game();
    g.players[P0].dev[DevCard::Alchemist] = 1;
    g.apply(P0, Action::PlayAlchemist { dice: (3, 4) }).unwrap();
    assert_eq!(g.dice, Some((3, 4)));
    assert_eq!(g.phase, Phase::MoveRobber);

    // Somebody holds too many: they discard first.
    let mut g = alchemist_game();
    g.players[P0].dev[DevCard::Alchemist] = 1;
    g.players[P1].hand = Res::new(3, 3, 2, 0, 0);
    g.apply(P0, Action::PlayAlchemist { dice: (6, 1) }).unwrap();
    assert_eq!(g.phase, Phase::Discard);
    assert_eq!(g.discards, vec![(P1, 4)]);
}

#[test]
fn the_alchemist_can_only_be_played_before_the_roll_by_the_player_whose_turn_it_is() {
    // After the roll
    let mut g = alchemist_game();
    g.players[P0].dev[DevCard::Alchemist] = 1;
    g.phase = Phase::Main;
    g.rolled = true;
    let err = g.apply(P0, Action::PlayAlchemist { dice: (3, 3) }).unwrap_err();
    assert!(err.contains("before you roll"), "{err}");
    assert_eq!(g.players[P0].dev[DevCard::Alchemist], 1, "a refused card is not used");

    // Out of turn
    let mut g = alchemist_game();
    g.players[P1].dev[DevCard::Alchemist] = 1;
    assert!(g.apply(P1, Action::PlayAlchemist { dice: (3, 3) }).is_err());
    assert_eq!(g.dice, None);
    assert_eq!(g.phase, Phase::Roll);
}

#[test]
fn the_alchemist_needs_the_card_and_is_one_development_card_a_turn() {
    // Not held
    let mut g = alchemist_game();
    assert!(g.apply(P0, Action::PlayAlchemist { dice: (3, 3) }).is_err());
    assert_eq!(g.phase, Phase::Roll);

    // Bought this turn: playable from the next one
    let mut g = alchemist_game();
    g.players[P0].dev_new[DevCard::Alchemist] = 1;
    assert!(g.apply(P0, Action::PlayAlchemist { dice: (3, 3) }).is_err());
    assert_eq!(g.dice, None);

    // A Knight has already been played this turn
    let mut g = alchemist_game();
    g.players[P0].dev[DevCard::Alchemist] = 1;
    g.players[P0].dev[DevCard::Knight] = 1;
    g.apply(P0, Action::PlayKnight).unwrap();
    let hex = g.legal(P0).hexes[0];
    g.apply(P0, Action::MoveRobber { hex }).unwrap();
    assert_eq!(g.phase, Phase::Roll, "back to the roll after the Knight");
    let err = g.apply(P0, Action::PlayAlchemist { dice: (3, 3) }).unwrap_err();
    assert!(err.contains("one development card"), "{err}");
    assert_eq!(g.players[P0].dev[DevCard::Alchemist], 1);
}

#[test]
fn the_dice_must_show_one_to_six() {
    for dice in [(0, 3), (3, 0), (7, 1), (1, 7), (0, 0), (255, 255)] {
        let mut g = alchemist_game();
        g.players[P0].dev[DevCard::Alchemist] = 1;
        assert!(g.apply(P0, Action::PlayAlchemist { dice }).is_err(), "{dice:?} was accepted");
        assert_eq!(g.players[P0].dev[DevCard::Alchemist], 1, "{dice:?} used the card");
        assert_eq!((g.phase, g.dice, g.dev_played), (Phase::Roll, None, false));
    }
    for dice in [(1, 1), (6, 6), (1, 6), (6, 1)] {
        let mut g = alchemist_game();
        g.players[P0].dev[DevCard::Alchemist] = 1;
        g.apply(P0, Action::PlayAlchemist { dice }).unwrap();
        assert_eq!(g.dice, Some(dice));
    }
}

#[test]
fn the_alchemist_is_refused_in_speed_mode() {
    let mut g = Game::with_settings(names(2), with(|s| s.speed = true));
    g.phase = Phase::Main;
    g.players[P0].dev[DevCard::Alchemist] = 1;
    let err = g.apply(P0, Action::PlayAlchemist { dice: (3, 3) }).unwrap_err();
    assert!(err.contains("Speed"), "{err}");
    assert_eq!(g.players[P0].dev[DevCard::Alchemist], 1);
}

#[test]
fn the_alchemist_is_a_card_you_hold_like_the_others() {
    let mut g = alchemist_game();
    g.players[P0].dev[DevCard::Alchemist] = 1;
    g.players[P0].dev_new[DevCard::Alchemist] = 1;
    let v = serde_json::to_value(g.view(P0, &all_connected(2), &g.longest_roads())).unwrap();
    assert_eq!(v["dev"], serde_json::json!([0, 0, 0, 0, 0, 1]), "six kinds, the Alchemist last");
    assert_eq!(v["dev_new"], serde_json::json!([0, 0, 0, 0, 0, 1]));
    assert_eq!(v["players"][0]["dev_cards"], 2);
    // everyone else sees only the count
    let v = serde_json::to_value(g.view(P1, &all_connected(2), &g.longest_roads())).unwrap();
    assert_eq!(v["players"][0]["dev_cards"], 2);
    assert_eq!(v["dev"], serde_json::json!([0, 0, 0, 0, 0, 0]));
}

#[test]
fn a_card_bought_this_turn_can_be_played_after_the_turn_ends() {
    let mut g = alchemist_game();
    g.players[P0].dev_new[DevCard::Alchemist] = 1;
    g.dice = Some((2, 2));
    g.phase = Phase::Main;
    g.rolled = true;
    g.apply(P0, Action::EndTurn).unwrap();
    // P1's turn (skipped to its main phase), then back to P0's
    assert_eq!(g.current, P1);
    g.phase = Phase::Main;
    g.rolled = true;
    g.apply(P1, Action::EndTurn).unwrap();
    assert_eq!(g.current, P0);
    assert_eq!(g.players[P0].dev[DevCard::Alchemist], 1, "ripe at last");
    g.apply(P0, Action::PlayAlchemist { dice: (4, 4) }).unwrap();
    assert_eq!(g.dice, Some((4, 4)));
}

// ----------------------------------------------------------------------
// The variable discard limit
// ----------------------------------------------------------------------

fn variable(limit: u8, step: u8, cap: u8) -> Settings {
    with(|s| {
        s.discard_limit = limit;
        s.variable_discard = true;
        s.discard_step_rolls = step;
        s.discard_limit_cap = cap;
    })
}

#[test]
fn the_limit_starts_where_it_was_set_goes_up_by_one_every_x_rolls_and_stops_at_the_cap() {
    let s = variable(7, 10, 9);
    let limits: Vec<u8> = [0, 1, 9, 10, 11, 19, 20, 21, 29, 30, 31, 1000, u32::MAX].iter().map(|&r| s.discard_limit_after(r)).collect();
    assert_eq!(limits, vec![7, 7, 7, 8, 8, 8, 9, 9, 9, 9, 9, 9, 9]);
    // every roll counts when x is 1
    let s = variable(4, 1, 6);
    assert_eq!((0..5).map(|r| s.discard_limit_after(r)).collect::<Vec<_>>(), vec![4, 5, 6, 6, 6]);
    // a cap equal to the start means it never rises
    assert_eq!(variable(7, 3, 7).discard_limit_after(300), 7);
}

#[test]
fn without_the_setting_the_limit_never_moves_whatever_the_other_two_say() {
    let s = with(|s| {
        s.variable_discard = false;
        s.discard_step_rolls = 1;
        s.discard_limit_cap = 50;
    });
    assert_eq!(s.discard_limit_after(0), 7);
    assert_eq!(s.discard_limit_after(500), 7);
}

#[test]
fn the_variable_limit_settings_have_ranges() {
    assert_eq!(variable(7, 10, 10).validate(), Ok(()));
    assert!(variable(7, 0, 10).validate().is_err(), "x of 0");
    assert!(with(|s| s.discard_step_rolls = 100).validate().is_err(), "x over 99");
    assert!(with(|s| s.discard_limit_cap = 0).validate().is_err());
    assert!(with(|s| s.discard_limit_cap = 51).validate().is_err());
    // a cap under the starting limit makes no sense while the limit is variable...
    let err = variable(8, 10, 7).validate().unwrap_err();
    assert!(err.contains("cap"), "{err}");
    // ...but is not worth refusing while it is off
    assert_eq!(with(|s| { s.discard_limit = 12; s.discard_limit_cap = 10 }).validate(), Ok(()));
}

/// A game in which `earlier` rolls have already been made, about to resolve one more roll of 7 (the way a roll does:
/// counted first, then carried out).
fn seven_after(earlier: u32, settings: Settings, hand: Res) -> Game {
    let mut g = Game::with_settings(names(2), settings);
    g.phase = Phase::Roll;
    g.current = P0;
    g.roll_counts[0] = earlier;
    g.players[P1].hand = hand;
    g.roll_dice_as((3, 4));
    g
}

#[test]
fn a_seven_is_held_to_the_limit_set_by_the_rolls_before_it() {
    // Starts at 4, up one every 3 rolls, at most 6. P1 holds 5 cards.
    let five = Res::new(2, 2, 1, 0, 0);
    let s = variable(4, 3, 6);
    // rolls 1, 2 and 3 use 4 (nothing has gone up yet): 5 cards is too many
    for earlier in [0, 1, 2] {
        let g = seven_after(earlier, s, five);
        assert_eq!(g.phase, Phase::Discard, "{} rolls before", earlier);
        assert_eq!(g.discards, vec![(P1, 2)]);
        assert!(g.log.contains(&LogEvent::MustDiscard { limit: 4 }));
    }
    // the 4th roll is the first to use 5: five cards are fine
    let g = seven_after(3, s, five);
    assert_eq!(g.phase, Phase::MoveRobber);
    // six cards still discard at 5, and are fine at the cap of 6
    let six = Res::new(2, 2, 2, 0, 0);
    let g = seven_after(5, s, six);
    assert_eq!(g.discards, vec![(P1, 3)]);
    assert!(g.log.contains(&LogEvent::MustDiscard { limit: 5 }));
    assert_eq!(seven_after(6, s, six).phase, Phase::MoveRobber);
    // and it climbs no higher: 7 cards discard however late the 7 comes
    let seven = Res::new(3, 2, 2, 0, 0);
    for earlier in [6, 60, 600] {
        let g = seven_after(earlier, s, seven);
        assert_eq!(g.discards, vec![(P1, 3)], "{earlier} rolls before");
        assert!(g.log.contains(&LogEvent::MustDiscard { limit: 6 }));
    }
}

#[test]
fn the_standard_limit_is_unchanged_when_the_setting_is_off() {
    let g = seven_after(500, with(|s| s.discard_limit = 4), Res::new(2, 2, 1, 0, 0));
    assert_eq!(g.phase, Phase::Discard);
    assert!(g.log.contains(&LogEvent::MustDiscard { limit: 4 }));
}

#[test]
fn clients_are_told_the_limit_that_applies_now() {
    let s = variable(4, 3, 6);
    let limit = |g: &Game| g.view(P0, &all_connected(2), &g.longest_roads()).discard_limit_now;
    let mut g = Game::with_settings(names(2), s);
    g.phase = Phase::Roll;
    g.current = P0;
    assert_eq!(limit(&g), 4, "at the start");
    g.roll_counts[0] = 2;
    assert_eq!(limit(&g), 4, "two rolls in: the next 7 is the third roll");
    g.roll_counts[0] = 3;
    assert_eq!(limit(&g), 5, "three rolls in: the next 7 is the first to be held to 5");

    // While the 7 is being dealt with the limit it was held to is shown, not the one for the next.
    let g = seven_after(2, s, Res::new(2, 2, 1, 0, 0));
    assert_eq!(g.phase, Phase::Discard);
    assert_eq!(limit(&g), 4, "the 7 that is being discarded for was the third roll");
    let mut g = g;
    g.apply(P1, Action::Discard { cards: Res::new(2, 0, 0, 0, 0) }).unwrap();
    assert_eq!(limit(&g), 5, "once it is dealt with, the next 7 is the fourth roll");

    // Without the setting it is the limit that was chosen
    let g = Game::with_settings(names(2), with(|s| s.discard_limit = 9));
    assert_eq!(limit(&g), 9);
}

#[test]
fn an_alchemist_roll_counts_toward_the_variable_limit() {
    let mut g = Game::with_settings(names(2), with(|s| {
        s.alchemist = true;
        s.variable_discard = true;
        s.discard_limit = 4;
        s.discard_step_rolls = 2;
        s.discard_limit_cap = 9;
    }));
    g.phase = Phase::Roll;
    g.current = P0;
    g.players[P0].dev[DevCard::Alchemist] = 1;
    g.apply(P0, Action::PlayAlchemist { dice: (1, 1) }).unwrap();
    assert_eq!(g.rolls(), 1);
    g.apply(P0, Action::EndTurn).unwrap();
    g.apply(P1, Action::Roll).unwrap();
    assert_eq!(g.rolls(), 2, "named and thrown dice are both rolls");
}
