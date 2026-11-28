//! The end-of-game stats and what is public while the game is on.

use crate::game::{Action, Building, Game, Phase, PlayerStats};
use crate::types::{DevCard, Res, Resource};
use super::support::{all_connected, build_on_volcano, names, P0, P1, P2, trading_game, volcano_game};

#[test]
fn dice_income_counts_the_cards_received_from_rolls() {
    let mut g = Game::new(names(2));
    let h = (0..g.board.hexes.len())
        .find(|&h| g.board.hexes[h].number.is_some() && g.board.hexes[h].terrain.resource().is_some())
        .expect("a producing hex");
    let roll = g.board.hexes[h].number.unwrap();
    let v = g.board.hexes[h].vertices[0];
    g.buildings[v] = Some(Building { owner: P0, city: false });
    g.distribute(roll);
    let first = g.players[P0].hand.total();
    assert!(first >= 1);
    assert_eq!(g.players[P0].stats.dice_income, first);
    assert_eq!(g.players[P1].stats.dice_income, 0);
    // a city pays double, and the total keeps adding up
    g.buildings[v] = Some(Building { owner: P0, city: true });
    g.distribute(roll);
    assert_eq!(g.players[P0].stats.dice_income, g.players[P0].hand.total());
    // cards that are not from a roll (a bank trade, say) are not income
    g.players[P0].hand = Res::new(4, 0, 0, 0, 0);
    let before = g.players[P0].stats.dice_income;
    g.phase = Phase::Main;
    g.current = P0;
    g.apply(P0, Action::BankTrade { give: Resource::Brick, get: Resource::Ore }).unwrap();
    assert_eq!(g.players[P0].stats.dice_income, before);
}

#[test]
fn volcano_picks_count_as_dice_income() {
    let mut g = volcano_game(2);
    build_on_volcano(&mut g, 0, P0, false); // 1 card
    build_on_volcano(&mut g, 2, P1, true); // 2 cards
    g.resolve_roll(8);
    while g.phase == Phase::VolcanoPick {
        let (q, _) = g.volcano_picks[0];
        g.apply(q, Action::VolcanoPick { resource: Resource::Grain }).unwrap();
    }
    assert_eq!(g.players[P0].stats.dice_income, 1);
    assert_eq!(g.players[P1].stats.dice_income, 2);
}

#[test]
fn buying_development_cards_is_counted_per_player() {
    let mut g = Game::new(names(2));
    g.phase = Phase::Main;
    g.current = P0;
    g.rolled = true;
    g.players[P0].hand = Res::new(0, 0, 2, 2, 2);
    g.apply(P0, Action::BuyDev).unwrap();
    g.apply(P0, Action::BuyDev).unwrap();
    assert_eq!(g.players[P0].stats.dev_cards_bought, 2);
    assert_eq!(g.players[P1].stats.dev_cards_bought, 0);
    // a refused purchase is not counted
    assert!(g.apply(P0, Action::BuyDev).is_err());
    assert_eq!(g.players[P0].stats.dev_cards_bought, 2);
}

#[test]
fn only_completed_player_trades_are_counted_for_both_sides() {
    let mut g = trading_game();
    g.apply(P0, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 1, 0, 0, 0) }).unwrap(); // 0
    g.apply(P0, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 0, 0, 1, 0) }).unwrap(); // 1
    g.apply(P1, Action::RespondTrade { id: 0, accept: true }).unwrap();
    // proposing and accepting alone is not a trade
    assert_eq!(g.players.iter().map(|p| p.stats.trades).collect::<Vec<_>>(), vec![0, 0, 0]);
    g.apply(P0, Action::ConfirmTrade { id: 0, with: P1 }).unwrap();
    assert_eq!(g.players.iter().map(|p| p.stats.trades).collect::<Vec<_>>(), vec![1, 1, 0]);
    // a withdrawn offer is not a trade
    g.apply(P0, Action::CancelTrade { id: 1 }).unwrap();
    assert_eq!(g.players.iter().map(|p| p.stats.trades).collect::<Vec<_>>(), vec![1, 1, 0]);
    // a bank trade is not a player trade
    g.players[P0].hand = Res::new(4, 0, 0, 0, 0);
    g.apply(P0, Action::BankTrade { give: Resource::Brick, get: Resource::Ore }).unwrap();
    assert_eq!(g.players[P0].stats.trades, 1);
}

#[test]
fn stats_are_revealed_only_when_the_game_is_over() {
    let mut g = Game::new(names(2));
    g.players[P0].stats = PlayerStats {
        dice_income: 7,
        dev_cards_bought: 2,
        trades: 3,
        robber_gained: 4,
        robber_lost: 1,
        dice_discarded: 6,
        blocked: 5,
        luck_36ths: 54, // 1.5 cards
    };
    g.players[P1].stats.luck_36ths = -5; // a fraction of a card is sent to two decimals
    let lens = g.longest_roads();
    let json = serde_json::to_value(g.view(P1, &all_connected(2), &lens)).unwrap();
    assert!(json["players"][0]["stats"].is_null(), "hidden during play");
    g.phase = Phase::GameOver;
    let json = serde_json::to_value(g.view(P1, &all_connected(2), &lens)).unwrap();
    assert_eq!(
        json["players"][0]["stats"],
        serde_json::json!({
            "dice_income": 7, "dev_cards_bought": 2, "trades": 3,
            "robber_gained": 4, "robber_lost": 1, "dice_discarded": 6, "blocked": 5, "luck": 1.5,
        })
    );
    assert_eq!(
        json["players"][1]["stats"],
        serde_json::json!({
            "dice_income": 0, "dev_cards_bought": 0, "trades": 0,
            "robber_gained": 0, "robber_lost": 0, "dice_discarded": 0, "blocked": 0, "luck": -0.14,
        })
    );
}

#[test]
fn dice_income_is_public_during_play_for_the_live_win_bar() {
    let mut g = Game::new(names(2));
    g.players[P0].stats = PlayerStats { dice_income: 7, dev_cards_bought: 2, trades: 3, ..PlayerStats::default() };
    g.players[P1].stats = PlayerStats { dice_income: 4, ..PlayerStats::default() };
    let lens = g.longest_roads();
    // every seat sees every player's income, in play and after the game ends; only the other stats stay hidden
    for you in [P0, P1] {
        let json = serde_json::to_value(g.view(you, &all_connected(2), &lens)).unwrap();
        assert_eq!(json["players"][0]["dice_income"], 7);
        assert_eq!(json["players"][1]["dice_income"], 4);
        assert!(json["players"][0]["stats"].is_null(), "the rest of the stats are still hidden during play");
    }
    g.phase = Phase::GameOver;
    let json = serde_json::to_value(g.view(P1, &all_connected(2), &lens)).unwrap();
    assert_eq!(json["players"][0]["dice_income"], 7);
    assert_eq!(json["players"][0]["stats"]["dice_income"], 7);
}

// ----------------------------------------------------------------------
// Robber+, Robber-, Dice-, Blocked and Luck
// ----------------------------------------------------------------------

/// A game whose only numbered hex is one that makes a resource; returns it with its number. P0 has a settlement
/// on one corner of it and P1 a city on another, and the robber is somewhere else.
fn lone_hex_game() -> (Game, usize, u8) {
    let mut g = Game::new(names(2));
    let h = (0..g.board.hexes.len())
        .find(|&h| h != g.robber && g.board.hexes[h].number.is_some() && g.board.hexes[h].terrain.resource().is_some())
        .expect("a producing hex");
    let number = g.board.hexes[h].number.unwrap();
    for other in 0..g.board.hexes.len() {
        if other != h {
            g.board.hexes[other].number = None;
        }
    }
    let (a, b) = (g.board.hexes[h].vertices[0], g.board.hexes[h].vertices[3]);
    g.buildings[a] = Some(Building { owner: P0, city: false });
    g.buildings[b] = Some(Building { owner: P1, city: true });
    g.phase = Phase::Roll;
    g.current = P0;
    (g, h, number)
}

/// In how many of the 36 throws of two dice the total is `n`.
fn ways(n: u8) -> i32 {
    6 - (7 - i32::from(n)).abs()
}

#[test]
fn a_steal_counts_one_card_gained_for_the_thief_and_lost_for_the_victim() {
    let mut g = Game::new(names(3));
    let h = (0..g.board.hexes.len()).find(|&h| h != g.robber).unwrap();
    let (a, b) = (g.board.hexes[h].vertices[0], g.board.hexes[h].vertices[2]);
    g.buildings[a] = Some(Building { owner: P1, city: false });
    g.players[P1].hand = Res::new(2, 1, 0, 0, 0);
    g.phase = Phase::MoveRobber;
    g.current = P0;
    // one possible victim: the card is taken straight away
    g.apply(P0, Action::MoveRobber { hex: h }).unwrap();
    assert_eq!(g.players[P1].hand.total(), 2);
    let stat = |g: &Game| g.players.iter().map(|p| (p.stats.robber_gained, p.stats.robber_lost)).collect::<Vec<_>>();
    assert_eq!(stat(&g), vec![(1, 0), (0, 1), (0, 0)]);

    // two possible victims: only the one chosen loses a card
    g.buildings[b] = Some(Building { owner: P2, city: true });
    g.players[P2].hand = Res::new(0, 0, 3, 0, 0);
    let elsewhere = (0..g.board.hexes.len()).find(|&x| x != g.robber && x != h).unwrap();
    g.phase = Phase::MoveRobber;
    g.robber = elsewhere;
    g.apply(P0, Action::MoveRobber { hex: h }).unwrap();
    assert_eq!(g.phase, Phase::Steal);
    assert_eq!(stat(&g), vec![(1, 0), (0, 1), (0, 0)], "nothing is taken until a victim is chosen");
    g.apply(P0, Action::Steal { victim: P2 }).unwrap();
    assert_eq!(stat(&g), vec![(2, 0), (0, 1), (0, 1)]);

    // a victim with no cards is not a victim: nothing is counted
    g.players[P1].hand = Res::ZERO;
    g.players[P2].hand = Res::ZERO;
    g.phase = Phase::MoveRobber;
    g.robber = elsewhere;
    g.apply(P0, Action::MoveRobber { hex: h }).unwrap();
    assert_eq!(stat(&g), vec![(2, 0), (0, 1), (0, 1)]);
    let (gained, lost): (u32, u32) = g.players.iter().fold((0, 0), |t, p| (t.0 + p.stats.robber_gained, t.1 + p.stats.robber_lost));
    assert_eq!(gained, lost, "every card taken is a card lost by someone");
}

#[test]
fn a_knight_steal_counts_like_any_other() {
    let mut g = Game::new(names(2));
    let h = (0..g.board.hexes.len()).find(|&h| h != g.robber).unwrap();
    let v = g.board.hexes[h].vertices[0];
    g.buildings[v] = Some(Building { owner: P1, city: false });
    g.players[P1].hand = Res::new(0, 0, 0, 0, 1);
    g.phase = Phase::Main;
    g.current = P0;
    g.rolled = true;
    g.players[P0].dev[DevCard::Knight] = 1;
    g.apply(P0, Action::PlayKnight).unwrap();
    g.apply(P0, Action::MoveRobber { hex: h }).unwrap();
    assert_eq!((g.players[P0].stats.robber_gained, g.players[P1].stats.robber_lost), (1, 1));
}

#[test]
fn cards_discarded_after_a_seven_are_counted_for_the_player_who_discarded() {
    let mut g = Game::new(names(2));
    g.phase = Phase::Roll;
    g.current = P0;
    g.players[P1].hand = Res::new(4, 4, 0, 0, 0); // 8 cards, one over the limit of 7: half (4) go
    g.resolve_roll(7);
    assert_eq!(g.phase, Phase::Discard);
    // a refused discard counts nothing
    assert!(g.apply(P1, Action::Discard { cards: Res::new(1, 0, 0, 0, 0) }).is_err());
    assert_eq!(g.players[P1].stats.dice_discarded, 0);
    g.apply(P1, Action::Discard { cards: Res::new(2, 2, 0, 0, 0) }).unwrap();
    assert_eq!(g.players[P1].stats.dice_discarded, 4);
    assert_eq!(g.players[P0].stats.dice_discarded, 0);
    // and it adds up over the game
    g.phase = Phase::Roll;
    g.players[P1].hand = Res::new(3, 3, 0, 0, 0); // 6 cards... one more than the limit of 5 that is set below
    g.settings.discard_limit = 5;
    g.resolve_roll(7);
    g.apply(P1, Action::Discard { cards: Res::new(2, 1, 0, 0, 0) }).unwrap();
    assert_eq!(g.players[P1].stats.dice_discarded, 7);
}

#[test]
fn a_hex_under_the_robber_counts_what_it_would_have_paid_as_blocked() {
    let (mut g, h, number) = lone_hex_game();
    g.robber = h;
    g.distribute(number);
    // P0's settlement would have paid 1 card, P1's city 2
    assert_eq!((g.players[P0].stats.blocked, g.players[P1].stats.blocked), (1, 2));
    assert_eq!((g.players[P0].stats.dice_income, g.players[P1].stats.dice_income), (0, 0));
    // a roll of another number blocks nothing
    g.distribute(if number == 4 { 5 } else { 4 });
    assert_eq!((g.players[P0].stats.blocked, g.players[P1].stats.blocked), (1, 2));
    // with the robber gone the hex pays, and nothing more is blocked
    g.robber = g.board.desert;
    g.distribute(number);
    assert_eq!((g.players[P0].stats.blocked, g.players[P1].stats.blocked), (1, 2));
    assert_eq!((g.players[P0].stats.dice_income, g.players[P1].stats.dice_income), (1, 2));
}

#[test]
fn a_volcano_under_the_robber_counts_the_picks_it_would_have_given_as_blocked() {
    let mut g = volcano_game(2);
    build_on_volcano(&mut g, 0, P0, false); // 1 card
    build_on_volcano(&mut g, 2, P1, true); // 2 cards
    for h in 1..g.board.hexes.len() {
        g.board.hexes[h].number = None;
    }
    g.robber = 0;
    g.resolve_roll(8);
    assert_eq!(g.phase, Phase::Main, "nobody picks anything");
    assert_eq!((g.players[P0].stats.blocked, g.players[P1].stats.blocked), (1, 2));
    assert_eq!((g.players[P0].stats.dice_income, g.players[P1].stats.dice_income), (0, 0));
}

#[test]
fn luck_is_the_dice_income_of_a_roll_minus_what_it_was_expected_to_bring() {
    let (mut g, _, number) = lone_hex_game();
    let w = ways(number);
    g.apply(P0, Action::Roll).unwrap(); // a random roll: whatever it is, work out what it must have been
    let total = g.dice.map(|(a, b)| a + b).unwrap();
    let got = if total == number { 1 } else { 0 };
    assert_eq!(g.players[P0].stats.luck_36ths, 36 * got - w, "a settlement is expected to bring {w}/36 of a card");
    assert_eq!(g.players[P1].stats.luck_36ths, 72 * got - 2 * w, "a city, twice that");
}

#[test]
fn a_seven_is_bad_luck_for_everyone_who_was_due_something() {
    let (mut g, _, number) = lone_hex_game();
    let w = ways(number);
    g.resolve_roll(7);
    assert_eq!(g.players[P0].stats.luck_36ths, -w);
    assert_eq!(g.players[P1].stats.luck_36ths, -2 * w);
    assert_eq!(g.players[P0].stats.dice_income, 0);
}

#[test]
fn over_all_36_throws_of_the_dice_luck_comes_out_even() {
    let (mut g, _, _) = lone_hex_game();
    for a in 1..=6u8 {
        for b in 1..=6u8 {
            g.phase = Phase::Roll;
            g.bank = Res::splat(g.settings.resource_max);
            g.players[P0].hand = Res::ZERO;
            g.players[P1].hand = Res::ZERO;
            g.roll_dice_as((a, b));
        }
    }
    assert_eq!(g.players[P0].stats.luck_36ths, 0);
    assert_eq!(g.players[P1].stats.luck_36ths, 0);
    assert!(g.players[P0].stats.dice_income > 0);
}

#[test]
fn the_robber_is_not_counted_as_bad_luck() {
    let (mut g, h, number) = lone_hex_game();
    g.robber = h;
    g.resolve_roll(number);
    // nothing was expected from a hex the robber sits on, and nothing came: that loss is `blocked`
    assert_eq!((g.players[P0].stats.luck_36ths, g.players[P1].stats.luck_36ths), (0, 0));
    assert!(g.players[P0].stats.blocked > 0);
}

#[test]
fn volcano_picks_are_part_of_the_luck_of_the_roll() {
    let mut g = volcano_game(2);
    for h in 1..g.board.hexes.len() {
        g.board.hexes[h].number = None;
    }
    build_on_volcano(&mut g, 0, P0, false);
    g.resolve_roll(8);
    while g.phase == Phase::VolcanoPick {
        let (q, _) = g.volcano_picks[0];
        g.apply(q, Action::VolcanoPick { resource: Resource::Grain }).unwrap();
    }
    assert_eq!(g.players[P0].stats.luck_36ths, 36 - ways(8), "one card against the 5/36 that was expected");
}

#[test]
fn a_roll_named_with_the_alchemist_is_nobodys_luck() {
    let (mut g, _, number) = lone_hex_game();
    g.settings.alchemist = true;
    g.players[P0].dev[DevCard::Alchemist] = 1;
    let dice = (number.saturating_sub(6).max(1), number - number.saturating_sub(6).max(1));
    g.apply(P0, Action::PlayAlchemist { dice }).unwrap();
    assert_eq!(g.players[P0].stats.dice_income, 1, "the roll paid");
    assert_eq!((g.players[P0].stats.luck_36ths, g.players[P1].stats.luck_36ths), (0, 0));
    // the next roll is a throw of the dice again, and counts
    g.phase = Phase::Roll;
    g.rolled = false;
    g.apply(P0, Action::Roll).unwrap();
    assert!(g.luck_counts);
    assert_ne!(g.players[P0].stats.luck_36ths, 0);
}

#[test]
fn income_that_is_not_from_a_roll_is_not_luck() {
    let mut g = Game::new(names(2));
    g.phase = Phase::Main;
    g.current = P0;
    g.players[P0].hand = Res::new(4, 0, 0, 0, 0);
    g.apply(P0, Action::BankTrade { give: Resource::Brick, get: Resource::Ore }).unwrap();
    assert_eq!(g.players[P0].stats.luck_36ths, 0);
}
