//! The small types the rules are written in: which resource, which development card, which player.
//!
//! They exist so that these three things can't be confused with each other, or with a hex, vertex or edge
//! number, which are all plain `usize`s. A `Resource` can only be one of the five, so nothing has to check
//! that an index is below 5. A `Res` (a bundle of cards) is indexed by `Resource`, and anything kept per seat
//! lives in a `PerPlayer`, which is indexed by `PlayerId` and cannot be indexed by a bare number.
//!
//! On the wire nothing changed: a `Resource` or a `DevCard` is its index (0 to 4), a `Res` or a `DevHand` is
//! an array of five numbers in index order, and a `PlayerId` is the seat number. The browser, which only ever
//! sees numbers, is unaffected.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::ops::{Index, IndexMut};

// ----------------------------------------------------------------------
// Resource and development card kinds
// ----------------------------------------------------------------------

/// Defines an enum of five kinds that serializes as its index, and a `$map` type holding one value per kind
/// that is indexed by the enum (and by nothing else).
macro_rules! five_kinds {
    (
        $(#[$kind_meta:meta])* $kind:ident { $($variant:ident => $name:literal),+ $(,)? }
        $(#[$map_meta:meta])* $map:ident
    ) => {
        $(#[$kind_meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $kind {
            $($variant),+
        }

        // The resources and the development cards get the same API; each uses a different part of it.
        #[allow(dead_code)]
        impl $kind {
            /// Every kind, in index order.
            pub const ALL: [$kind; 5] = [$($kind::$variant),+];

            /// Position in `ALL`, which is also what the browser calls it.
            pub fn index(self) -> usize {
                self as usize
            }

            /// The kind at `index`, if there is one.
            pub fn from_index(index: usize) -> Option<$kind> {
                Self::ALL.get(index).copied()
            }

            /// Lower-case name, for messages.
            pub fn name(self) -> &'static str {
                match self {
                    $($kind::$variant => $name),+
                }
            }
        }

        impl Serialize for $kind {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_u8(self.index() as u8)
            }
        }

        impl<'de> Deserialize<'de> for $kind {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let n = u64::deserialize(d)?;
                usize::try_from(n)
                    .ok()
                    .and_then($kind::from_index)
                    .ok_or_else(|| D::Error::custom(format!("{n} is not a {}", stringify!($kind))))
            }
        }

        $(#[$map_meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $map<T>(pub [T; 5]);

        impl<T> Index<$kind> for $map<T> {
            type Output = T;
            fn index(&self, kind: $kind) -> &T {
                &self.0[kind.index()]
            }
        }

        impl<T> IndexMut<$kind> for $map<T> {
            fn index_mut(&mut self, kind: $kind) -> &mut T {
                &mut self.0[kind.index()]
            }
        }

        #[allow(dead_code)]
        impl<T> $map<T> {
            /// One value per kind, made by `f`.
            pub fn from_fn(f: impl FnMut($kind) -> T) -> Self {
                $map($kind::ALL.map(f))
            }

            /// Each kind with its value, in index order.
            pub fn iter(&self) -> impl Iterator<Item = ($kind, &T)> {
                $kind::ALL.into_iter().zip(self.0.iter())
            }
        }

        #[allow(dead_code)]
        impl<T: Copy> $map<T> {
            /// The same value for every kind.
            pub fn splat(value: T) -> Self {
                $map([value; 5])
            }
        }
    };
}

five_kinds! {
    /// One of the five resources.
    Resource { Brick => "brick", Lumber => "lumber", Ore => "ore", Grain => "grain", Wool => "wool" }
    /// One value of type `T` for each resource.
    ByResource
}

five_kinds! {
    /// One of the five kinds of development card.
    DevCard {
        Knight => "knight",
        RoadBuilding => "road building",
        YearOfPlenty => "year of plenty",
        Monopoly => "monopoly",
        VictoryPoint => "victory point",
    }
    /// One value of type `T` for each kind of development card.
    ByDev
}

/// A bundle of resource cards: a hand, the bank, a price, one side of a trade.
pub type Res = ByResource<u8>;

impl Res {
    pub const ZERO: Res = ByResource([0; 5]);

    /// In the order brick, lumber, ore, grain, wool.
    pub const fn new(brick: u8, lumber: u8, ore: u8, grain: u8, wool: u8) -> Res {
        ByResource([brick, lumber, ore, grain, wool])
    }

    /// How many cards in all.
    pub fn total(&self) -> u32 {
        self.0.iter().map(|&n| n as u32).sum()
    }

    /// Whether this bundle holds at least as much of every resource as `need`.
    pub fn covers(&self, need: &Res) -> bool {
        Resource::ALL.iter().all(|&r| self[r] >= need[r])
    }
}

/// How many development cards of each kind a player holds.
pub type DevHand = ByDev<u8>;

impl DevHand {
    pub const ZERO: DevHand = ByDev([0; 5]);

    /// How many cards in all.
    pub fn total(&self) -> u32 {
        self.0.iter().map(|&n| n as u32).sum()
    }
}

// ----------------------------------------------------------------------
// Players
// ----------------------------------------------------------------------

/// A player: the seat number, which is also the player's place in `PerPlayer` collections. (It says
/// nothing about turn order, which is `Game::order`.)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlayerId(usize);

impl PlayerId {
    pub const fn new(seat: usize) -> PlayerId {
        PlayerId(seat)
    }

    /// The seat number.
    pub fn index(self) -> usize {
        self.0
    }

    /// The players of a table of `n`, in seat order.
    pub fn range(n: usize) -> impl Iterator<Item = PlayerId> + Clone {
        (0..n).map(PlayerId)
    }
}

/// One `T` for each seat, indexed by `PlayerId` and nothing else, so a hex number can't be used by mistake
/// to look up a player.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct PerPlayer<T>(Vec<T>);

impl<T> PerPlayer<T> {
    pub fn new() -> PerPlayer<T> {
        PerPlayer(Vec::new())
    }

    /// One value for each of `n` players, made by `f`.
    pub fn from_fn(n: usize, f: impl FnMut(PlayerId) -> T) -> PerPlayer<T> {
        PerPlayer(PlayerId::range(n).map(f).collect())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, p: PlayerId) -> Option<&T> {
        self.0.get(p.0)
    }

    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.0.iter()
    }

    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.0.iter_mut()
    }

    /// Each player with their value, in seat order.
    pub fn entries(&self) -> impl Iterator<Item = (PlayerId, &T)> {
        self.0.iter().enumerate().map(|(i, t)| (PlayerId(i), t))
    }

    /// The seat of the first value `pred` accepts.
    pub fn position(&self, pred: impl FnMut(&T) -> bool) -> Option<PlayerId> {
        self.0.iter().position(pred).map(PlayerId)
    }

    pub fn push(&mut self, value: T) {
        self.0.push(value);
    }

    /// Remove a seat; the seats after it move up by one.
    pub fn remove(&mut self, p: PlayerId) -> T {
        self.0.remove(p.0)
    }

    pub fn retain(&mut self, keep: impl FnMut(&T) -> bool) {
        self.0.retain(keep);
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

impl<T> Default for PerPlayer<T> {
    fn default() -> Self {
        PerPlayer::new()
    }
}

impl<T> From<Vec<T>> for PerPlayer<T> {
    fn from(values: Vec<T>) -> Self {
        PerPlayer(values)
    }
}

impl<T> FromIterator<T> for PerPlayer<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        PerPlayer(iter.into_iter().collect())
    }
}

impl<T> Index<PlayerId> for PerPlayer<T> {
    type Output = T;
    fn index(&self, p: PlayerId) -> &T {
        &self.0[p.0]
    }
}

impl<T> IndexMut<PlayerId> for PerPlayer<T> {
    fn index_mut(&mut self, p: PlayerId) -> &mut T {
        &mut self.0[p.0]
    }
}

impl<'a, T> IntoIterator for &'a PerPlayer<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn kinds_have_a_fixed_order_and_round_trip_through_their_index() {
        assert_eq!(Resource::ALL.map(Resource::index), [0, 1, 2, 3, 4]);
        assert_eq!(DevCard::ALL.map(DevCard::index), [0, 1, 2, 3, 4]);
        for r in Resource::ALL {
            assert_eq!(Resource::from_index(r.index()), Some(r));
        }
        assert_eq!(Resource::from_index(5), None);
        assert_eq!(DevCard::from_index(5), None);
        assert_eq!(Resource::Brick.name(), "brick");
        assert_eq!(DevCard::VictoryPoint.name(), "victory point");
    }

    #[test]
    fn the_wire_format_is_the_same_numbers_and_arrays_as_before() {
        assert_eq!(serde_json::to_value(Resource::Ore).unwrap(), json!(2));
        assert_eq!(serde_json::to_value(DevCard::Monopoly).unwrap(), json!(3));
        assert_eq!(serde_json::to_value(Res::new(1, 0, 2, 0, 3)).unwrap(), json!([1, 0, 2, 0, 3]));
        assert_eq!(serde_json::to_value(DevHand::ZERO).unwrap(), json!([0, 0, 0, 0, 0]));
        assert_eq!(serde_json::to_value(PlayerId::new(3)).unwrap(), json!(3));
        let seats: PerPlayer<u8> = vec![7, 8].into();
        assert_eq!(serde_json::to_value(&seats).unwrap(), json!([7, 8]));
    }

    #[test]
    fn what_a_client_sends_is_read_the_same_way_and_a_bad_resource_is_refused_when_it_is_read() {
        assert_eq!(serde_json::from_value::<Resource>(json!(4)).unwrap(), Resource::Wool);
        assert_eq!(serde_json::from_value::<Res>(json!([0, 1, 2, 3, 4])).unwrap(), Res::new(0, 1, 2, 3, 4));
        assert_eq!(serde_json::from_value::<PlayerId>(json!(2)).unwrap(), PlayerId::new(2));
        // Out of range, negative, fractional, wrong shape: none of these can become a Resource.
        for bad in [json!(5), json!(-1), json!(1.5), json!("brick"), json!(null), json!(u64::MAX)] {
            assert!(serde_json::from_value::<Resource>(bad.clone()).is_err(), "{bad} was accepted");
            assert!(serde_json::from_value::<DevCard>(bad.clone()).is_err(), "{bad} was accepted");
        }
        assert!(serde_json::from_value::<Res>(json!([1, 2, 3])).is_err());
        assert!(serde_json::from_value::<Res>(json!([1, 2, 3, 4, 5, 6])).is_err());
    }

    #[test]
    fn bundles_count_and_compare() {
        let hand = Res::new(2, 1, 0, 3, 0);
        assert_eq!(hand.total(), 6);
        assert!(hand.covers(&Res::new(1, 1, 0, 3, 0)));
        assert!(!hand.covers(&Res::new(0, 0, 1, 0, 0)));
        assert!(hand.covers(&Res::ZERO));
        let mut mine = Res::ZERO;
        mine[Resource::Wool] += 2;
        assert_eq!(mine, Res::new(0, 0, 0, 0, 2));
        assert_eq!(Res::from_fn(|r| r.index() as u8), Res::new(0, 1, 2, 3, 4));
        assert_eq!(Res::splat(4).total(), 20);
        assert_eq!(hand.iter().map(|(r, &n)| (r, n)).nth(3), Some((Resource::Grain, 3)));
    }

    #[test]
    fn per_player_collections_are_indexed_by_player() {
        let mut names: PerPlayer<String> = PerPlayer::from_fn(3, |p| format!("P{}", p.index()));
        assert_eq!(names.len(), 3);
        assert_eq!(names[PlayerId::new(2)], "P2");
        names[PlayerId::new(1)].push('!');
        assert_eq!(names.get(PlayerId::new(1)).map(String::as_str), Some("P1!"));
        assert_eq!(names.get(PlayerId::new(3)), None);
        assert_eq!(names.position(|n| n == "P1!"), Some(PlayerId::new(1)));
        assert_eq!(names.entries().map(|(p, _)| p.index()).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(PlayerId::range(3).count(), 3);
        names.remove(PlayerId::new(0));
        assert_eq!(names.len(), 2);
        names.retain(|n| n != "P2");
        assert_eq!(names.len(), 1);
        names.clear();
        assert!(names.is_empty());
    }
}
