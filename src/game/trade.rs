//! Exchanging cards: the harbours and bank trades, and the offers players make to each other.

use crate::log_event::LogEvent;
use crate::types::{ByResource, PerPlayer, PlayerId, Res, Resource};
use serde::Serialize;
use super::{Game, HarbourView, Phase};

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Reply {
    Proposer,
    Pending,
    Accepted,
    /// The offer is a bank-rate deal for this player, who owns the harbour that gives it (see
    /// `bank_rate_deal`), so they cannot accept it. They can still decline it.
    Blocked,
    Declined,
}

/// The most player offers one proposer may have open at once.
pub const MAX_OPEN_TRADES: usize = 4;

/// Why a harbour owner's offer or acceptance is refused.
const HARBOUR_DEAL_MSG: &str = "A harbour owner can't make bank-rate deals with players (3 for 1 with a 3:1 harbour, 2 for 1 with a matching 2:1 harbour, or several of those at once). Trade with the bank instead";

/// Whether giving `give` and getting `get_total` cards back is nothing but bank trades done with another
/// player. `any` is whether the giver owns a 3:1 harbour and `special` which resources they own a 2:1 harbour
/// for. Every resource given must be a whole number of 3s (with `any`) or 2s (with its own harbour), each such
/// group buying one card, and the groups must add up to exactly `get_total`. So 3 lumber for 1 brick and
/// 6 lumber for 2 brick are bank deals with a 3:1 harbour, but 3 lumber + 1 ore for 1 brick, 3 lumber for 2 brick,
/// and a 4:1 deal are not. With no harbour nothing is.
pub fn bank_rate_deal(give: &Res, get_total: u32, any: bool, special: &ByResource<bool>) -> bool {
    if get_total == 0 || (!any && !special.0.iter().any(|&s| s)) {
        return false;
    }
    // `reach[k]`: the cards bought if the resources looked at so far are split into harbour groups so that k
    // cards come back.
    let size = give.total() as usize / 2 + 1;
    let mut reach = vec![false; size + 1];
    reach[0] = true;
    for r in Resource::ALL {
        let n = give[r] as usize;
        if n == 0 {
            continue;
        }
        let mut options = vec![];
        if special[r] && n % 2 == 0 {
            options.push(n / 2);
        }
        if any && n % 3 == 0 {
            options.push(n / 3);
        }
        if options.is_empty() {
            return false; // a resource that doesn't fit a harbour ratio is not a bank deal
        }
        let mut next = vec![false; size + 1];
        for (have, &ok) in reach.iter().enumerate() {
            if ok {
                for &add in &options {
                    if have + add <= size {
                        next[have + add] = true;
                    }
                }
            }
        }
        reach = next;
    }
    (get_total as usize) <= size && reach[get_total as usize]
}

#[derive(Clone, Debug, Serialize)]
pub struct Trade {
    /// Unique for the whole game; replies, confirmations and withdrawals name an offer by it.
    pub id: u32,
    pub from: PlayerId,
    /// What the proposer hands over.
    pub give: Res,
    /// What the proposer wants in return.
    pub want: Res,
    pub replies: PerPlayer<Reply>,
}

impl Game {
    /// Whether `p` has a settlement or city on a harbour of this kind: `None` for 3:1 any resource,
    /// `Some(r)` for the 2:1 harbour of `r`.
    fn owns_harbour(&self, p: PlayerId, kind: Option<Resource>) -> bool {
        self.board.ports.iter().any(|port| {
            port.resource == kind
                && [port.a, port.b]
                    .iter()
                    .any(|&v| matches!(self.buildings[v], Some(b) if b.owner == p))
        })
    }

    /// The harbours `p` owns.
    pub fn harbours(&self, p: PlayerId) -> HarbourView {
        HarbourView {
            any: self.owns_harbour(p, None),
            resource: ByResource::from_fn(|r| self.owns_harbour(p, Some(r))),
        }
    }

    /// Best trade ratio `p` has for giving `res` to the bank: the base ratio from the
    /// settings (4 by default), improved by harbours to 3 or 2 where that is better.
    pub fn ratio(&self, p: PlayerId, res: Resource) -> u8 {
        let mut r = self.settings.bank_ratio;
        if self.owns_harbour(p, None) {
            r = r.min(3);
        }
        if self.owns_harbour(p, Some(res)) {
            r = r.min(2);
        }
        r
    }

    /// Whether `p` handing `give` to another player for `get` would be a bank-rate deal that one of `p`'s
    /// harbours makes (see `bank_rate_deal`). Harbour owners must use the bank for those.
    pub fn bank_rate_deal(&self, p: PlayerId, give: &Res, get: &Res) -> bool {
        let h = self.harbours(p);
        bank_rate_deal(give, get.total(), h.any, &h.resource)
    }

    pub(super) fn do_bank_trade(&mut self, p: PlayerId, give: Resource, get: Resource) -> Result<(), String> {
        if self.phase != Phase::Main {
            return Err("You can only trade after rolling".into());
        }
        if give == get {
            return Err("Pick two different resources".into());
        }
        let ratio = self.ratio(p, give);
        if self.players[p].hand[give] < ratio {
            return Err(format!("You need {} {} for that trade", ratio, give.name()));
        }
        if self.bank[get] == 0 {
            return Err(format!("The bank has no {} left", get.name()));
        }
        self.players[p].hand[give] -= ratio;
        self.bank[give] += ratio;
        self.bank[get] -= 1;
        self.players[p].hand[get] += 1;
        self.log(LogEvent::BankTrade { player: p, give, get, ratio });
        self.add_bonus();
        Ok(())
    }

    /// Open another offer to every other player
    pub(super) fn do_offer_trade(&mut self, p: PlayerId, give: Res, want: Res) -> Result<(), String> {
        if self.phase != Phase::Main {
            return Err("You can only trade after rolling".into());
        }
        if give.total() == 0 || want.total() == 0 {
            return Err("A trade needs cards on both sides".into());
        }
        if Resource::ALL.iter().any(|&r| give[r] > 0 && want[r] > 0) {
            return Err("You can't offer and ask for the same resource".into());
        }
        if !self.players[p].hand.covers(&give) {
            return Err("You don't have those cards".into());
        }
        if self.bank_rate_deal(p, &give, &want) {
            return Err(HARBOUR_DEAL_MSG.into());
        }
        if self.trades.iter().any(|t| t.from == p && t.give == give && t.want == want) {
            return Err("You already have that offer open".into());
        }
        if self.trades.iter().filter(|t| t.from == p).count() >= MAX_OPEN_TRADES {
            return Err(format!("You can have at most {MAX_OPEN_TRADES} offers open at once"));
        }
        let mut replies = PerPlayer::from_fn(self.n, |q| {
            if self.bank_rate_deal(q, &want, &give) {
                Reply::Blocked
            } else {
                Reply::Pending
            }
        });
        replies[p] = Reply::Proposer;
        let id = self.next_trade_id;
        self.next_trade_id += 1;
        self.trades.push(Trade {
            id,
            from: p,
            give,
            want,
            replies,
        });
        self.log(LogEvent::TradeProposed { player: p });
        Ok(())
    }

    pub(super) fn do_respond_trade(&mut self, p: PlayerId, id: u32, accept: bool) -> Result<(), String> {
        let hand = self.players[p].hand;
        let (from, give, want) = self
            .trades
            .iter()
            .find(|t| t.id == id)
            .map(|t| (t.from, t.give, t.want))
            .ok_or("That offer is no longer open")?;
        if from == p {
            return Err("You proposed this trade".into());
        }
        if accept {
            if !hand.covers(&want) {
                return Err("You don't have the cards to accept".into());
            }
            // `p` would hand over `want` and get `give`.
            if self.bank_rate_deal(p, &want, &give) {
                return Err(HARBOUR_DEAL_MSG.into());
            }
        }
        if let Some(t) = self.trades.iter_mut().find(|t| t.id == id) {
            t.replies[p] = if accept { Reply::Accepted } else { Reply::Declined };
        }
        Ok(())
    }

    /// Complete offer `id` with a player who accepted it. The proposer's other offers stay
    /// open; `prune_trades` then drops any the proposer can no longer pay for.
    pub(super) fn do_confirm_trade(&mut self, p: PlayerId, id: u32, with: PlayerId) -> Result<(), String> {
        let t = self.trades.iter().find(|t| t.id == id).cloned().ok_or("That offer is no longer open")?;
        if t.from != p {
            return Err("Only the proposer can confirm".into());
        }
        if !self.has_player(with) || t.replies[with] != Reply::Accepted {
            return Err("That player hasn't accepted".into());
        }
        if !self.players[p].hand.covers(&t.give) || !self.players[with].hand.covers(&t.want) {
            return Err("One of you no longer has the cards for this trade".into());
        }
        if self.bank_rate_deal(p, &t.give, &t.want) || self.bank_rate_deal(with, &t.want, &t.give) {
            return Err(HARBOUR_DEAL_MSG.into());
        }
        for r in Resource::ALL {
            if self.settings.speed {
                self.players[p].traded_away[r] = self.players[p].traded_away[r].saturating_add(t.give[r]);
            } else {
                self.traded_away[r] = self.traded_away[r].saturating_add(t.give[r]);
            }
            self.players[p].hand[r] = self.players[p].hand[r] - t.give[r] + t.want[r];
            self.players[with].hand[r] = self.players[with].hand[r] + t.give[r] - t.want[r];
        }
        self.trades.retain(|o| o.id != id);
        self.players[p].stats.trades += 1;
        self.players[with].stats.trades += 1;
        self.log(LogEvent::TradeCompleted { player: p, with });
        self.add_bonus();
        Ok(())
    }

    pub(super) fn do_cancel_trade(&mut self, p: PlayerId, id: u32) -> Result<(), String> {
        match self.trades.iter().find(|t| t.id == id) {
            None => return Err("That offer is no longer open".into()),
            Some(t) if t.from != p => return Err("That isn't your offer".into()),
            Some(_) => {}
        }
        self.trades.retain(|t| t.id != id);
        Ok(())
    }

    /// Keep the open offers honest after any change to the hands (a completed trade, a bank
    /// trade, building, buying a card): an offer the proposer can no longer pay for is
    /// withdrawn, and a player whose acceptance they can no longer cover is asked again.
    /// Offers only exist during the proposer's own main phase.
    pub(super) fn prune_trades(&mut self) {
        if self.trades.is_empty() {
            return;
        }
        if self.phase != Phase::Main {
            self.trades.clear();
            return;
        }
        // An offer is withdrawn once its proposer can no longer pay for it, or once building on a harbour has
        // turned it into a bank deal for them.
        let mut trades = std::mem::take(&mut self.trades);
        trades.retain(|t| self.players[t.from].hand.covers(&t.give) && !self.bank_rate_deal(t.from, &t.give, &t.want));
        let players = &self.players;
        for t in &mut trades {
            for q in PlayerId::range(players.len()) {
                if t.replies[q] == Reply::Accepted && !players[q].hand.covers(&t.want) {
                    t.replies[q] = Reply::Pending;
                }
            }
        }
        if self.settings.speed {
            // Anybody can build on a harbour at any moment, so who is blocked from a bank-rate deal can change
            // while an offer is open. (An answer already given is left alone; `do_respond_trade` and
            // `do_confirm_trade` check the harbours again anyway.)
            for t in &mut trades {
                for q in PlayerId::range(self.n) {
                    if q != t.from && matches!(t.replies[q], Reply::Pending | Reply::Blocked) {
                        t.replies[q] = if self.bank_rate_deal(q, &t.want, &t.give) { Reply::Blocked } else { Reply::Pending };
                    }
                }
            }
        }
        // (In a classic game blocked replies are set when an offer opens: harbours only change on their owner's turn.)
        self.trades = trades;
    }
}
