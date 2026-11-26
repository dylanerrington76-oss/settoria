//! Offers between players, bank-rate deals for harbour owners, and the Monopoly lock.

use crate::game::trade::{bank_rate_deal, MAX_OPEN_TRADES};
use crate::game::{Action, Building, Game, Reply};
use crate::types::{ByResource, DevCard, PlayerId, Res, Resource};
use super::support::{all_connected, P0, P1, P2, trading_game};

#[test]
fn several_offers_can_be_open_at_once_and_are_answered_separately() {
    let mut g = trading_game();
    g.apply(P0, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 1, 0, 0, 0) }).unwrap();
    g.apply(P0, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 0, 1, 0, 0) }).unwrap();
    g.apply(P0, Action::OfferTrade { give: Res::new(2, 0, 0, 0, 0), want: Res::new(0, 0, 0, 1, 0) }).unwrap();
    assert_eq!(g.trades.len(), 3);
    let ids: Vec<u32> = g.trades.iter().map(|t| t.id).collect();
    assert_eq!(ids, vec![0, 1, 2]);

    // player 1 declines the first and accepts the second; player 2 accepts the third
    g.apply(P1, Action::RespondTrade { id: 0, accept: false }).unwrap();
    g.apply(P1, Action::RespondTrade { id: 1, accept: true }).unwrap();
    g.apply(P2, Action::RespondTrade { id: 2, accept: true }).unwrap();
    assert_eq!(g.trades[0].replies[P1], Reply::Declined);
    assert_eq!(g.trades[0].replies[P2], Reply::Pending, "replies are per offer");
    assert_eq!(g.trades[1].replies[P1], Reply::Accepted);
    assert_eq!(g.trades[1].replies[P0], Reply::Proposer);

    // the proposer withdraws one; the others are untouched
    g.apply(P0, Action::CancelTrade { id: 0 }).unwrap();
    assert_eq!(g.trades.iter().map(|t| t.id).collect::<Vec<_>>(), vec![1, 2]);
    assert!(g.apply(P0, Action::CancelTrade { id: 0 }).is_err(), "already gone");
    assert!(g.apply(P1, Action::RespondTrade { id: 0, accept: true }).is_err(), "can't answer a gone offer");
}

#[test]
fn identical_offers_and_too_many_offers_are_refused() {
    let mut g = trading_game();
    g.players[P0].hand = Res::new(9, 0, 0, 0, 0);
    let offer = Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 1, 0, 0, 0) };
    g.apply(P0, offer.clone()).unwrap();
    assert!(g.apply(P0, offer).is_err(), "same give and want twice");
    for k in 2..=MAX_OPEN_TRADES as u8 {
        g.apply(P0, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, k, 0, 0, 0) }).unwrap();
    }
    assert_eq!(g.trades.len(), MAX_OPEN_TRADES);
    assert!(g.apply(P0, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 0, 0, 0, 1) }).is_err());
}

#[test]
fn completing_one_trade_keeps_the_others_open_and_drops_the_unaffordable() {
    let mut g = trading_game();
    // Offers A and B each need 2 of player 0's 3 brick, so only one can ever complete.
    // C needs 1 brick and stays payable after A completes.
    g.apply(P0, Action::OfferTrade { give: Res::new(2, 0, 0, 0, 0), want: Res::new(0, 1, 0, 0, 0) }).unwrap(); // A = 0
    g.apply(P0, Action::OfferTrade { give: Res::new(2, 0, 0, 0, 0), want: Res::new(0, 0, 1, 0, 0) }).unwrap(); // B = 1
    g.apply(P0, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 0, 0, 1, 0) }).unwrap(); // C = 2
    g.apply(P1, Action::RespondTrade { id: 0, accept: true }).unwrap();
    g.apply(P1, Action::RespondTrade { id: 1, accept: true }).unwrap();
    g.apply(P2, Action::RespondTrade { id: 2, accept: true }).unwrap();

    g.apply(P0, Action::ConfirmTrade { id: 0, with: P1 }).unwrap();
    assert_eq!(g.players[P0].hand, Res::new(1, 1, 0, 0, 0));
    assert_eq!(g.players[P1].hand, Res::new(2, 1, 2, 0, 0));
    // B needs 2 brick and only 1 is left: withdrawn. C survives.
    assert_eq!(g.trades.iter().map(|t| t.id).collect::<Vec<_>>(), vec![2]);
    g.apply(P0, Action::ConfirmTrade { id: 2, with: P2 }).unwrap();
    assert_eq!(g.players[P0].hand, Res::new(0, 1, 0, 1, 0));
    assert!(g.trades.is_empty());
}

#[test]
fn an_accepter_who_can_no_longer_pay_is_asked_again() {
    let mut g = trading_game();
    // Player 1 accepts two offers that both ask for 2 ore; completing one uses it up.
    g.apply(P0, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 0, 2, 0, 0) }).unwrap(); // 0
    g.apply(P0, Action::OfferTrade { give: Res::new(2, 0, 0, 0, 0), want: Res::new(0, 0, 2, 0, 0) }).unwrap(); // 1
    g.apply(P1, Action::RespondTrade { id: 0, accept: true }).unwrap();
    g.apply(P1, Action::RespondTrade { id: 1, accept: true }).unwrap();
    g.apply(P0, Action::ConfirmTrade { id: 0, with: P1 }).unwrap();
    assert_eq!(g.trades.len(), 1);
    assert_eq!(g.trades[0].replies[P1], Reply::Pending, "player 1 spent the ore, so must answer again");
    assert!(g.apply(P0, Action::ConfirmTrade { id: 1, with: P1 }).is_err());
}

#[test]
fn a_bank_trade_withdraws_offers_the_proposer_can_no_longer_pay() {
    let mut g = trading_game();
    g.players[P0].hand = Res::new(4, 1, 1, 1, 1);
    g.apply(P0, Action::OfferTrade { give: Res::new(4, 0, 0, 0, 0), want: Res::new(0, 0, 1, 0, 0) }).unwrap();
    g.apply(P0, Action::OfferTrade { give: Res::new(0, 1, 0, 0, 0), want: Res::new(0, 0, 1, 0, 0) }).unwrap();
    // 4:1 with the bank spends all the brick
    g.apply(P0, Action::BankTrade { give: Resource::Brick, get: Resource::Ore }).unwrap();
    assert_eq!(g.trades.len(), 1);
    assert_eq!(g.trades[0].give, Res::new(0, 1, 0, 0, 0));
}

/// Give `p` a settlement on a harbour of this kind (`None` = 3:1).
fn give_harbour(g: &mut Game, p: PlayerId, kind: Option<Resource>) {
    let port = g
        .board
        .ports
        .iter()
        .find(|port| port.resource == kind)
        .expect("the standard board has this harbour")
        .clone();
    g.buildings[port.a] = Some(Building { owner: p, city: false });
}

fn offer(g: &mut Game, p: PlayerId, give: Res, want: Res) -> Result<(), String> {
    g.apply(p, Action::OfferTrade { give, want })
}

#[test]
fn bank_rate_deals_are_recognised() {
    let none = ByResource::splat(false);
    let lumber = ByResource::from_fn(|r| r == Resource::Lumber);
    let deal = |give: Res, get: u32, any: bool, special: &ByResource<bool>| bank_rate_deal(&give, get, any, special);
    // no harbour: nothing is a bank deal, whatever the ratio
    assert!(!deal(Res::new(4, 0, 0, 0, 0), 1, false, &none));
    assert!(!deal(Res::new(3, 0, 0, 0, 0), 1, false, &none));
    // 3:1 harbour: 3 for 1 and its multiples
    assert!(deal(Res::new(3, 0, 0, 0, 0), 1, true, &none));
    assert!(deal(Res::new(6, 0, 0, 0, 0), 2, true, &none));
    assert!(deal(Res::new(3, 3, 0, 0, 0), 2, true, &none), "two 3:1 deals in one");
    assert!(!deal(Res::new(6, 0, 0, 0, 0), 1, true, &none), "6 for 1 is worse than the bank, so it is allowed");
    assert!(!deal(Res::new(6, 0, 0, 0, 0), 3, true, &none), "6 for 3 is better than the bank");
    assert!(!deal(Res::new(3, 0, 0, 0, 0), 2, true, &none));
    assert!(!deal(Res::new(2, 0, 0, 0, 0), 1, true, &none));
    assert!(!deal(Res::new(4, 0, 0, 0, 0), 1, true, &none));
    assert!(!deal(Res::new(3, 1, 0, 0, 0), 1, true, &none), "a leftover card makes it a different deal");
    assert!(!deal(Res::new(2, 1, 0, 0, 0), 1, true, &none), "mixed cards are not a bank deal");
    assert!(!deal(Res::ZERO, 1, true, &none));
    assert!(!deal(Res::new(3, 0, 0, 0, 0), 0, true, &none));
    // 2:1 harbour for lumber only
    assert!(deal(Res::new(0, 2, 0, 0, 0), 1, false, &lumber));
    assert!(deal(Res::new(0, 4, 0, 0, 0), 2, false, &lumber));
    assert!(!deal(Res::new(2, 0, 0, 0, 0), 1, false, &lumber), "a 2:1 harbour only covers its own resource");
    assert!(!deal(Res::new(0, 3, 0, 0, 0), 1, false, &lumber));
    assert!(!deal(Res::new(0, 1, 0, 0, 0), 1, false, &lumber));
    // both: lumber counts in 2s or 3s, other resources in 3s
    assert!(deal(Res::new(0, 6, 0, 0, 0), 2, true, &lumber), "as 3:1");
    assert!(deal(Res::new(0, 6, 0, 0, 0), 3, true, &lumber), "as 2:1");
    assert!(deal(Res::new(3, 2, 0, 0, 0), 2, true, &lumber), "one 3:1 and one 2:1");
    assert!(!deal(Res::new(2, 2, 0, 0, 0), 2, true, &lumber), "brick has no 2:1 and 2 is not a multiple of 3");
    assert!(!deal(Res::new(0, 6, 0, 0, 0), 4, true, &lumber));
}

#[test]
fn a_3_to_1_harbour_owner_cannot_offer_players_a_bank_deal() {
    let mut g = trading_game();
    give_harbour(&mut g, P0, None);
    g.players[P0].hand = Res::new(9, 3, 0, 0, 0);
    assert!(offer(&mut g, P0, Res::new(3, 0, 0, 0, 0), Res::new(0, 0, 1, 0, 0)).is_err(), "3 for 1");
    assert!(offer(&mut g, P0, Res::new(6, 0, 0, 0, 0), Res::new(0, 0, 1, 1, 0)).is_err(), "6 for 2");
    assert!(g.trades.is_empty());
    // everything that is not exactly a bank deal is still allowed
    offer(&mut g, P0, Res::new(2, 0, 0, 0, 0), Res::new(0, 0, 1, 0, 0)).unwrap();
    offer(&mut g, P0, Res::new(4, 0, 0, 0, 0), Res::new(0, 0, 1, 0, 0)).unwrap();
    offer(&mut g, P0, Res::new(3, 0, 0, 0, 0), Res::new(0, 0, 2, 0, 0)).unwrap();
    offer(&mut g, P0, Res::new(2, 1, 0, 0, 0), Res::new(0, 0, 1, 0, 0)).unwrap();
    assert_eq!(g.trades.len(), 4);
}

#[test]
fn a_2_to_1_harbour_only_blocks_its_own_resource() {
    let mut g = trading_game();
    give_harbour(&mut g, P0, Some(Resource::Brick));
    g.players[P0].hand = Res::new(4, 4, 0, 0, 0);
    assert!(offer(&mut g, P0, Res::new(2, 0, 0, 0, 0), Res::new(0, 0, 1, 0, 0)).is_err(), "2 brick for 1");
    assert!(offer(&mut g, P0, Res::new(4, 0, 0, 0, 0), Res::new(0, 0, 1, 1, 0)).is_err(), "4 brick for 2");
    offer(&mut g, P0, Res::new(0, 2, 0, 0, 0), Res::new(0, 0, 1, 0, 0)).unwrap();
    offer(&mut g, P0, Res::new(3, 0, 0, 0, 0), Res::new(0, 0, 1, 0, 0)).unwrap();
    assert_eq!(g.trades.len(), 2);
}

#[test]
fn without_a_harbour_player_trades_are_unrestricted() {
    let mut g = trading_game();
    g.players[P0].hand = Res::new(4, 3, 2, 0, 0);
    offer(&mut g, P0, Res::new(4, 0, 0, 0, 0), Res::new(0, 0, 0, 1, 0)).unwrap();
    offer(&mut g, P0, Res::new(0, 3, 0, 0, 0), Res::new(0, 0, 0, 1, 0)).unwrap();
    offer(&mut g, P0, Res::new(0, 0, 2, 0, 0), Res::new(0, 0, 0, 0, 1)).unwrap();
}

#[test]
fn a_harbour_owner_cannot_accept_a_bank_deal_but_others_can() {
    let mut g = trading_game();
    give_harbour(&mut g, P1, None); // player 1: 3:1
    g.players[P0].hand = Res::new(0, 0, 1, 0, 0);
    g.players[P1].hand = Res::new(3, 0, 0, 0, 0);
    g.players[P2].hand = Res::new(3, 0, 0, 0, 0);
    // player 0 offers 1 ore for 3 brick: for players 1 (3:1) that is a bank deal, for player 2 it is not
    offer(&mut g, P0, Res::new(0, 0, 1, 0, 0), Res::new(3, 0, 0, 0, 0)).unwrap();
    assert_eq!(g.trades[0].replies[P1], Reply::Blocked);
    assert_eq!(g.trades[0].replies[P2], Reply::Pending);
    assert_eq!(g.trades[0].replies[P0], Reply::Proposer);
    assert!(g.apply(P1, Action::RespondTrade { id: 0, accept: true }).is_err());
    assert_eq!(g.trades[0].replies[P1], Reply::Blocked, "a refused answer changes nothing");
    g.apply(P2, Action::RespondTrade { id: 0, accept: true }).unwrap();
    assert!(g.apply(P0, Action::ConfirmTrade { id: 0, with: P1 }).is_err());
    g.apply(P0, Action::ConfirmTrade { id: 0, with: P2 }).unwrap();
    assert_eq!(g.players[P0].hand, Res::new(3, 0, 0, 0, 0));
}

#[test]
fn a_blocked_player_can_still_decline() {
    let mut g = trading_game();
    give_harbour(&mut g, P1, Some(Resource::Ore));
    g.players[P0].hand = Res::new(1, 0, 0, 0, 0);
    g.players[P1].hand = Res::new(0, 0, 2, 0, 0);
    offer(&mut g, P0, Res::new(1, 0, 0, 0, 0), Res::new(0, 0, 2, 0, 0)).unwrap(); // player 1 would give 2 ore for 1 brick
    assert_eq!(g.trades[0].replies[P1], Reply::Blocked);
    g.apply(P1, Action::RespondTrade { id: 0, accept: false }).unwrap();
    assert_eq!(g.trades[0].replies[P1], Reply::Declined);
}

#[test]
fn building_on_a_harbour_withdraws_offers_that_become_bank_deals() {
    let mut g = trading_game();
    g.players[P0].hand = Res::new(3, 0, 0, 0, 0);
    offer(&mut g, P0, Res::new(3, 0, 0, 0, 0), Res::new(0, 0, 1, 0, 0)).unwrap();
    offer(&mut g, P0, Res::new(2, 0, 0, 0, 0), Res::new(0, 0, 1, 0, 0)).unwrap();
    give_harbour(&mut g, P0, None);
    g.prune_trades();
    assert_eq!(g.trades.len(), 1);
    assert_eq!(g.trades[0].give, Res::new(2, 0, 0, 0, 0));
}

#[test]
fn giving_away_two_cards_in_trades_locks_monopoly_on_them_for_the_turn() {
    let mut g = trading_game();
    g.players[P0].hand = Res::new(3, 0, 0, 0, 0);
    g.players[P0].dev[DevCard::Monopoly] = 1;
    g.players[P1].hand = Res::new(0, 2, 2, 0, 0);
    offer(&mut g, P0, Res::new(2, 0, 0, 0, 0), Res::new(0, 1, 0, 0, 0)).unwrap();
    g.apply(P1, Action::RespondTrade { id: 0, accept: true }).unwrap();
    g.apply(P0, Action::ConfirmTrade { id: 0, with: P1 }).unwrap();
    assert!(g.monopoly_locked(Resource::Brick));
    assert!(!g.monopoly_locked(Resource::Lumber), "receiving cards doesn't lock them");
    let v = g.view(P0, &all_connected(3), &g.longest_roads());
    assert_eq!(v.monopoly_locked, ByResource::from_fn(|r| r == Resource::Brick));
    let v = g.view(P1, &all_connected(3), &g.longest_roads());
    assert_eq!(v.monopoly_locked, ByResource::splat(false), "only the player whose turn it is has a lock");
    let err = g.apply(P0, Action::PlayMonopoly { resource: Resource::Brick }).unwrap_err();
    assert!(err.contains("Monopoly"), "{err}");
    assert_eq!(g.players[P0].dev[DevCard::Monopoly], 1, "a refused Monopoly doesn't use the card");
    assert!(!g.dev_played);
    // another resource is fine
    g.apply(P0, Action::PlayMonopoly { resource: Resource::Ore }).unwrap();
    // the lock ends with the turn
    g.apply(P0, Action::EndTurn).unwrap();
    assert_eq!(g.traded_away, Res::ZERO);
    assert!(!g.monopoly_locked(Resource::Brick));
}

#[test]
fn small_and_bank_trades_do_not_lock_monopoly_but_a_running_total_of_two_does() {
    let mut g = trading_game();
    g.players[P0].hand = Res::new(8, 0, 0, 0, 0);
    g.players[P0].dev[DevCard::Monopoly] = 1;
    g.players[P1].hand = Res::new(0, 2, 2, 0, 0);
    // trading the bank 4 brick doesn't count
    g.apply(P0, Action::BankTrade { give: Resource::Brick, get: Resource::Wool }).unwrap();
    assert!(!g.monopoly_locked(Resource::Brick));
    // one brick to a player is under the limit
    offer(&mut g, P0, Res::new(1, 0, 0, 0, 0), Res::new(0, 1, 0, 0, 0)).unwrap();
    g.apply(P1, Action::RespondTrade { id: 0, accept: true }).unwrap();
    g.apply(P0, Action::ConfirmTrade { id: 0, with: P1 }).unwrap();
    assert!(!g.monopoly_locked(Resource::Brick));
    // a second one makes two in all
    offer(&mut g, P0, Res::new(1, 0, 0, 0, 0), Res::new(0, 0, 1, 0, 0)).unwrap();
    g.apply(P1, Action::RespondTrade { id: 1, accept: true }).unwrap();
    g.apply(P0, Action::ConfirmTrade { id: 1, with: P1 }).unwrap();
    assert!(g.monopoly_locked(Resource::Brick));
    assert!(g.apply(P0, Action::PlayMonopoly { resource: Resource::Brick }).is_err());
}

#[test]
fn declined_offers_stay_visible_to_the_proposer_as_declined() {
    let mut g = trading_game();
    g.apply(P0, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 1, 0, 0, 0) }).unwrap();
    g.apply(P1, Action::RespondTrade { id: 0, accept: false }).unwrap();
    let v = g.view(P0, &all_connected(3), &g.longest_roads());
    assert_eq!(v.trades.len(), 1);
    assert_eq!(v.trades[0].replies[P1], Reply::Declined);
    let json = serde_json::to_string(&v.trades).unwrap();
    assert!(json.contains("\"id\":0") && json.contains("\"declined\""), "{json}");
}

#[test]
fn offers_end_with_the_turn_and_with_playing_a_development_card() {
    let mut g = trading_game();
    g.players[P0].dev[DevCard::Monopoly] = 1;
    g.apply(P0, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 1, 0, 0, 0) }).unwrap();
    g.apply(P0, Action::PlayMonopoly { resource: Resource::Lumber }).unwrap();
    assert!(g.trades.is_empty());
    g.apply(P0, Action::OfferTrade { give: Res::new(1, 0, 0, 0, 0), want: Res::new(0, 0, 1, 0, 0) }).unwrap();
    g.apply(P0, Action::EndTurn).unwrap();
    assert!(g.trades.is_empty());
}
