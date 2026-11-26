//! The end-of-game stats and what is public while the game is on.

use crate::game::{Action, Building, Game, Phase, PlayerStats};
use crate::types::{Res, Resource};
use super::support::{all_connected, build_on_volcano, names, P0, P1, trading_game, volcano_game};

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
    g.players[P0].stats = PlayerStats { dice_income: 7, dev_cards_bought: 2, trades: 3 };
    let lens = g.longest_roads();
    let json = serde_json::to_value(g.view(P1, &all_connected(2), &lens)).unwrap();
    assert!(json["players"][0]["stats"].is_null(), "hidden during play");
    g.phase = Phase::GameOver;
    let json = serde_json::to_value(g.view(P1, &all_connected(2), &lens)).unwrap();
    assert_eq!(json["players"][0]["stats"], serde_json::json!({ "dice_income": 7, "dev_cards_bought": 2, "trades": 3 }));
    assert_eq!(json["players"][1]["stats"], serde_json::json!({ "dice_income": 0, "dev_cards_bought": 0, "trades": 0 }));
}

#[test]
fn dice_income_is_public_during_play_for_the_live_win_bar() {
    let mut g = Game::new(names(2));
    g.players[P0].stats = PlayerStats { dice_income: 7, dev_cards_bought: 2, trades: 3 };
    g.players[P1].stats = PlayerStats { dice_income: 4, dev_cards_bought: 0, trades: 0 };
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
