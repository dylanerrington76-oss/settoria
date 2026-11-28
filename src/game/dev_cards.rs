//! Development cards: buying them, playing them, and the Monopoly lock that trading can put on a resource.

use crate::log_event::LogEvent;
use crate::types::{DevCard, PlayerId, Res, Resource};
use super::{Game, Phase};

pub const COST_DEV: Res = Res::new(0, 0, 1, 1, 1);

/// Cards of one resource handed to other players in trades during one turn that lock that resource out of
/// Monopoly for the rest of the turn.
pub const MONOPOLY_LOCK_CARDS: u8 = 2;

impl Game {
    /// Whether `r` is locked out of Monopoly for the current player: they gave away `MONOPOLY_LOCK_CARDS` or
    /// more of it to other players in trades this turn.
    pub fn monopoly_locked(&self, r: Resource) -> bool {
        self.monopoly_locked_for(self.current, r)
    }

    /// Whether `p` is locked out of Monopoly on `r`: in a classic game that is the current player (the only one
    /// who can trade); in Speed mode every player has a lock of their own that the next roll clears.
    pub fn monopoly_locked_for(&self, p: PlayerId, r: Resource) -> bool {
        let given = if self.settings.speed { self.players[p].traded_away[r] } else if p == self.current { self.traded_away[r] } else { 0 };
        given >= MONOPOLY_LOCK_CARDS
    }

    /// Whether `p` has played a development card in the current turn (Speed mode: since the last roll).
    pub fn dev_played_by(&self, p: PlayerId) -> bool {
        if self.settings.speed {
            self.players[p].dev_played
        } else {
            self.dev_played
        }
    }

    pub(super) fn do_buy_dev(&mut self, p: PlayerId) -> Result<(), String> {
        if self.phase != Phase::Main {
            return Err("You can't buy a development card right now".into());
        }
        if self.deck.is_empty() {
            return Err("The development deck is empty".into());
        }
        if !self.players[p].hand.covers(&COST_DEV) {
            return Err("A development card costs 1 ore, 1 grain and 1 wool".into());
        }
        self.pay(p, &COST_DEV);
        let card = self.deck.pop().unwrap();
        self.players[p].dev_new[card] += 1;
        self.players[p].stats.dev_cards_bought += 1;
        self.log(LogEvent::BoughtDevCard { player: p });
        self.add_bonus();
        Ok(())
    }

    fn check_dev_play(&self, p: PlayerId, kind: DevCard) -> Result<(), String> {
        let allowed = if self.settings.speed { self.phase == Phase::Main } else { matches!(self.phase, Phase::Roll | Phase::Main) };
        if !allowed {
            return Err("You can't play a development card right now".into());
        }
        if self.dev_played_by(p) {
            return Err(if self.settings.speed {
                "You can only play one development card between two rolls".into()
            } else {
                "You can only play one development card per turn".into()
            });
        }
        if self.players[p].dev[kind] == 0 {
            return Err("You don't have that card, or you bought it this turn".into());
        }
        Ok(())
    }

    fn use_dev(&mut self, p: PlayerId, kind: DevCard) {
        self.players[p].dev[kind] -= 1;
        if self.settings.speed {
            // Only the player's own offers are withdrawn: everybody else's are still on the table.
            self.players[p].dev_played = true;
            self.trades.retain(|t| t.from != p);
        } else {
            self.dev_played = true;
            self.trades.clear();
        }
    }

    pub(super) fn do_knight(&mut self, p: PlayerId) -> Result<(), String> {
        self.check_dev_play(p, DevCard::Knight)?;
        self.use_dev(p, DevCard::Knight);
        self.players[p].knights += 1;
        self.log(LogEvent::PlayedKnight { player: p });
        self.update_largest_army(p);
        if self.settings.speed {
            // The knight's owner places this robber (the rotation of 7s is not touched); the table waits for them.
            self.current = p;
        }
        self.phase = Phase::MoveRobber;
        Ok(())
    }

    pub(super) fn do_road_building_card(&mut self, p: PlayerId) -> Result<(), String> {
        self.check_dev_play(p, DevCard::RoadBuilding)?;
        if self.legal_roads(p).is_empty() {
            return Err("You have nowhere to build a road".into());
        }
        self.use_dev(p, DevCard::RoadBuilding);
        if self.settings.speed {
            // Nobody else has to wait: the two free roads are simply credited, to be placed whenever the player likes.
            self.players[p].free_roads = self.players[p].roads_left.min(2);
        } else {
            self.road_building_left = self.players[p].roads_left.min(2);
            self.phase = Phase::RoadBuilding;
        }
        self.log(LogEvent::PlayedRoadBuilding { player: p });
        Ok(())
    }

    pub(super) fn do_year_of_plenty(&mut self, p: PlayerId, a: Resource, b: Resource) -> Result<(), String> {
        self.check_dev_play(p, DevCard::YearOfPlenty)?;
        let ok = if a == b {
            self.bank[a] >= 2
        } else {
            self.bank[a] >= 1 && self.bank[b] >= 1
        };
        if !ok {
            return Err("The bank doesn't have those resources".into());
        }
        self.use_dev(p, DevCard::YearOfPlenty);
        self.bank[a] -= 1;
        self.bank[b] -= 1;
        self.players[p].hand[a] += 1;
        self.players[p].hand[b] += 1;
        self.log(LogEvent::PlayedYearOfPlenty { player: p, first: a, second: b });
        Ok(())
    }

    /// The Alchemist: played at the start of the turn, before the dice are rolled, it takes the place of the
    /// roll. Its owner says what each die shows, and the roll is carried out exactly as if the dice had fallen
    /// that way (production, or the 7 sequence; it counts as a roll for `Game::roll_counts` and the variable
    /// discard limit). It uses up the turn's one development card like any other.
    pub(super) fn do_alchemist(&mut self, p: PlayerId, dice: (u8, u8)) -> Result<(), String> {
        if self.settings.speed {
            return Err("The Alchemist can't be played in Speed mode: there are no turns to start".into());
        }
        if self.phase != Phase::Roll {
            return Err("The Alchemist is played at the start of your turn, before you roll the dice".into());
        }
        self.check_dev_play(p, DevCard::Alchemist)?;
        if !(1..=6).contains(&dice.0) || !(1..=6).contains(&dice.1) {
            return Err("Each die must show a number from 1 to 6".into());
        }
        self.use_dev(p, DevCard::Alchemist);
        self.log(LogEvent::PlayedAlchemist { player: p });
        // The owner chose these dice, so the roll is neither lucky nor unlucky for anyone.
        self.luck_counts = false;
        self.roll_dice_as(dice);
        Ok(())
    }

    pub(super) fn do_monopoly(&mut self, p: PlayerId, r: Resource) -> Result<(), String> {
        self.check_dev_play(p, DevCard::Monopoly)?;
        if self.monopoly_locked_for(p, r) {
            let given = if self.settings.speed { self.players[p].traded_away[r] } else { self.traded_away[r] };
            return Err(format!(
                "You gave away {} {} in trades this turn, so you can't play Monopoly on it until your next {}",
                given,
                r.name(),
                if self.settings.speed { "roll" } else { "turn" }
            ));
        }
        self.use_dev(p, DevCard::Monopoly);
        let mut taken = 0u32;
        for o in PlayerId::range(self.n) {
            if o != p {
                let c = self.players[o].hand[r];
                self.players[o].hand[r] = 0;
                self.players[p].hand[r] += c;
                taken += c as u32;
            }
        }
        self.log(LogEvent::PlayedMonopoly { player: p, resource: r, taken });
        Ok(())
    }
}
