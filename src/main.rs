mod board;
mod bot;
mod builtin;
mod game;
mod log_event;
mod map_def;
mod map_files;
mod map_loader;
mod maps;
mod patch;
mod types;
mod validate;

use axum::{
    body::Bytes,
    extract::{
        rejection::BytesRejection,
        ws::{Message, WebSocket, WebSocketUpgrade},
        DefaultBodyLimit, State,
    },
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use game::{palette_color, Action, Game, Phase, Settings, TimerMode, PALETTE};
use log_event::LogEvent;
use maps::SharedMaps;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tower_http::services::{ServeDir, ServeFile};
use types::{PerPlayer, PlayerId};

type Tx = UnboundedSender<String>;
type Shared = Arc<Mutex<Room>>;

/// Seats at the table (the engine's limit; a map can allow fewer, see `PlayerRange`).
const MAX_PLAYERS: usize = map_def::MAX_PLAYERS;

/// The host is whoever sits in the first seat; every host-only message checks for it.
const HOST: PlayerId = PlayerId::new(0);

/// Messages for joining and managing the room (everything else is a game `Action`).
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Control {
    /// `delta` asks for patches instead of a whole view after every action (see `update_seat`). Left out, the
    /// seat is sent a whole view each time, as it always was, so clients that know nothing of patches still work.
    Join {
        name: String,
        color: Option<String>,
        token: Option<String>,
        #[serde(default)]
        delta: bool,
    },
    /// A client that was sent patches and has lost track of them asks for the whole view again.
    Resync,
    Start,
    /// Host only, before the game starts: pick the turn timer mode.
    SetTimer { mode: TimerMode },
    /// Host only, before the game starts: replace the game rules (the whole `Settings` object).
    SetSettings { settings: Settings },
    /// Host only, before the game starts: pick the map, by the id the lobby lists it under.
    SetMap { map: String },
    AddBot,
    /// Host only, before the game starts: take the bot in seat `seat` away again.
    RemoveBot { seat: usize },
    /// After a game ends: return everyone to the lobby for a new game.
    Reset,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ClientMsg {
    Control(Control),
    Action(Action),
}

#[derive(Serialize)]
struct LobbyPlayer {
    name: String,
    color: String,
    /// A bot always counts as connected: there is nobody behind it to lose.
    connected: bool,
    bot: bool,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMsg<'a> {
    Welcome { token: &'a str },
    /// `map` is the id of the chosen map; `maps` is every map the host can choose from.
    Lobby { you: PlayerId, players: &'a [LobbyPlayer], timer: TimerMode, settings: Settings, map: &'a str, maps: &'a [maps::MapInfo] },
    /// The whole view, board and log included. A seat that asked for patches is sent one first (`seq` 0) and
    /// again whenever it asks (`resync`) or reconnects; it counts the patches after it from there. A seat that
    /// didn't ask is sent one each time, with no `seq`.
    Game {
        #[serde(skip_serializing_if = "Option::is_none")]
        seq: Option<u64>,
        view: &'a game::GameView<'a>,
    },
    /// What changed in the view since the last message to this seat: `ops` (see patch.rs) for everything but
    /// the board and the log, and the log events the seat hasn't got, numbered from `log_from`. `seq` is one
    /// more than the last message's, so a client can tell it missed one.
    Patch {
        seq: u64,
        ops: Vec<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        log_from: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        log: Option<&'a [LogEvent]>,
    },
    /// `code` lets the client react to specific failures (e.g. `color_taken`).
    Error {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<&'static str>,
    },
}

struct Seat {
    name: String,
    color: &'static str,
    token: String,
    tx: Option<Tx>,
    /// Whether the client asked for patches (`delta` in its `join`).
    delta: bool,
    /// What this seat has been sent of the game in progress, if it asks for patches. `None` until it has been
    /// sent a whole view, and again whenever it must be sent one afresh (it reconnects, asks to resync, or the
    /// game ends).
    synced: Option<Synced>,
    /// A computer player (`bot.rs`). It has no connection (`tx` is `None`) and its token is never given to anyone,
    /// but it is not "away": the game plays it itself (see `Room::tick_at`).
    bot: bool,
}

/// What a seat that asks for patches has been sent, so the next message can be the difference.
struct Synced {
    /// The seat's view as the client now holds it, as JSON, without the board and the log: those are never
    /// patched (the board never changes; the log is sent by number, see `log_end`).
    state: serde_json::Value,
    /// The `seq` of the last message sent.
    seq: u64,
    /// One more than the number of the last log event sent (events are numbered from 0 for the whole game).
    log_end: usize,
    /// When the countdown in `state` was last sent; see `steady_timer`.
    timer_at: Instant,
}

/// How far a countdown may be from where the client has counted it to before the server sends the time again.
/// The server checks its clocks every 200 ms, and a message takes a moment to build.
const TIMER_SLACK_MS: u64 = 500;

/// A view's `timer.remaining_ms` is measured when the view is built, so it differs in every view even when
/// nothing about the clock has happened, and would put an op in every patch. The client counts down by itself,
/// so a countdown that is where the client has counted it to is left out: `next`'s `remaining_ms` is put back
/// to what was last sent, and the diff then finds nothing. A different clock (another kind or player, a new
/// total), a bonus or a clock that appeared or went is sent as it is. Returns when the countdown in `next` was
/// last sent: `sent_at` if it wasn't sent again, else `now`.
fn steady_timer(prev: &serde_json::Value, next: &mut serde_json::Value, sent_at: Instant, now: Instant) -> Instant {
    let (Some(old), Some(new)) = (prev.get("timer"), next.get_mut("timer")) else { return now };
    let remaining = |t: &serde_json::Value| t.get("remaining_ms").and_then(serde_json::Value::as_u64);
    let (Some(was), Some(is)) = (remaining(old), remaining(new)) else { return now };
    let same_clock = ["kind", "player", "total_ms"].iter().all(|k| old.get(*k) == new.get(*k));
    let elapsed = now.saturating_duration_since(sent_at).as_millis() as u64;
    if same_clock && is.abs_diff(was.saturating_sub(elapsed)) <= TIMER_SLACK_MS {
        if let (Some(fields), Some(was)) = (new.as_object_mut(), old.get("remaining_ms")) {
            fields.insert("remaining_ms".to_string(), was.clone());
        }
        sent_at
    } else {
        now
    }
}

/// Tell one seat how the game stands. A seat that didn't ask for patches is sent the whole view. One that
/// did is sent the whole view the first time (the board in it is the only time it is ever sent), and after
/// that only what changed since the last message, or nothing at all if nothing did. `view` is for that seat.
fn update_seat(seat: &mut Seat, mut view: game::GameView<'_>) {
    let Some(tx) = seat.tx.clone() else { return };
    if !seat.delta {
        return send(&tx, &ServerMsg::Game { seq: None, view: &view });
    }
    let log = view.log;
    let log_start = view.log_start;
    let log_end = log_start + log.len();
    // A seat that is missing events the view has already dropped from its window can't be patched (a patch
    // can't say "you missed some"), so it starts again. That takes more than a window's worth of events
    // between two messages, which a turn never makes.
    let resumable = matches!(&seat.synced, Some(s) if s.log_end >= log_start);
    let whole = if resumable { None } else { Some(serde_json::to_string(&ServerMsg::Game { seq: Some(0), view: &view })) };

    // What patches are made of: the view without its board and its log.
    view.board = None;
    view.log = &[];
    let Ok(mut state) = serde_json::to_value(&view) else { return };
    if let Some(fields) = state.as_object_mut() {
        fields.remove("log");
        fields.remove("log_start");
    }
    let now = Instant::now();

    if let Some(whole) = whole {
        if let Ok(text) = whole {
            let _ = tx.send(text);
        }
        seat.synced = Some(Synced { state, seq: 0, log_end, timer_at: now });
        return;
    }
    let Some(synced) = seat.synced.as_mut() else { return };
    let timer_at = steady_timer(&synced.state, &mut state, synced.timer_at, now);
    let mut ops = Vec::new();
    patch::diff(&synced.state, &state, &mut ops);
    let fresh = &log[(synced.log_end - log_start).min(log.len())..];
    if ops.is_empty() && fresh.is_empty() {
        return;
    }
    synced.seq += 1;
    let msg = ServerMsg::Patch {
        seq: synced.seq,
        ops,
        log_from: if fresh.is_empty() { None } else { Some(log_end - fresh.len()) },
        log: if fresh.is_empty() { None } else { Some(fresh) },
    };
    synced.state = state;
    synced.log_end = log_end;
    synced.timer_at = timer_at;
    send(&tx, &msg);
}

/// The single in-memory table. Nothing here is ever written to disk.
struct Room {
    seats: PerPlayer<Seat>,
    game: Option<Game>,
    /// Chosen by the host in the lobby; used when the game starts.
    timer: TimerMode,
    /// Rules chosen by the host in the lobby; used when the game starts.
    settings: Settings,
    /// Map chosen by the host in the lobby, by id; used when the game starts.
    map: String,
    /// Every map the server can play. Shared, with a lock of its own, so looking one up never waits on this
    /// room's lock for long and never makes the room wait on it.
    maps: SharedMaps,
    /// Moves the bots' seats in the game in progress, at a pace people can follow. Started afresh with each game.
    bots: bot::Driver,
}

impl Default for Room {
    fn default() -> Room {
        Room {
            seats: PerPlayer::new(),
            game: None,
            timer: TimerMode::default(),
            settings: Settings::default(),
            map: builtin::DEFAULT_MAP.to_string(),
            maps: maps::shared_with_builtins(),
            bots: bot::Driver::new(Instant::now(), &mut rand::thread_rng()),
        }
    }
}

fn send(tx: &Tx, msg: &ServerMsg) {
    if let Ok(s) = serde_json::to_string(msg) {
        let _ = tx.send(s);
    }
}

fn error(tx: &Tx, message: impl Into<String>) {
    send(tx, &ServerMsg::Error { message: message.into(), code: None });
}

impl Room {
    fn color_taken(&self, color: &str) -> bool {
        self.seats.iter().any(|s| s.color == color)
    }

    fn seat_of(&self, token: &str) -> Option<PlayerId> {
        self.seats.position(|s| s.token == token)
    }

    /// Who is connected, by seat. A bot is always there.
    fn connected(&self) -> PerPlayer<bool> {
        self.seats.iter().map(|s| s.tx.is_some() || s.bot).collect()
    }

    /// The host is whoever sits in the first seat, and a bot can't be host (it can't start the game or choose the
    /// map). So when the host leaves and a bot would be first, the people move up ahead of the bots, keeping their
    /// order. Nothing changes otherwise.
    fn humans_first(&mut self) {
        if !self.seats.get(HOST).map_or(false, |s| s.bot) {
            return;
        }
        let mut all = Vec::new();
        while !self.seats.is_empty() {
            all.push(self.seats.remove(HOST));
        }
        all.sort_by_key(|s| s.bot); // stable: people first, each group in the order it sat
        for seat in all {
            self.seats.push(seat);
        }
    }

    /// Tell everyone how the table stands: the lobby, or the game (see `update_seat` for what a seat is sent).
    fn broadcast(&mut self) {
        let ids: Vec<PlayerId> = self.seats.entries().map(|(i, _)| i).collect();
        match &self.game {
            Some(g) => {
                let connected = self.connected();
                // Longest road is the same in every seat's view and costly to compute, so do it once here.
                let road_lens = g.longest_roads();
                for i in ids {
                    update_seat(&mut self.seats[i], g.view(i, &connected, &road_lens));
                }
            }
            None => {
                // Only a lobby lists the maps (a game just carries its board), so only now ask the registry.
                let maps = maps::infos(&self.maps);
                let players: Vec<LobbyPlayer> = self
                    .seats
                    .iter()
                    .map(|s| LobbyPlayer { name: s.name.clone(), color: s.color.to_string(), connected: s.tx.is_some() || s.bot, bot: s.bot })
                    .collect();
                for i in ids {
                    let seat = &mut self.seats[i];
                    // Whatever game comes next starts with a whole view, board and all.
                    seat.synced = None;
                    let Some(tx) = &seat.tx else { continue };
                    send(
                        tx,
                        &ServerMsg::Lobby { you: i, timer: self.timer, settings: self.settings, map: &self.map, maps: &maps, players: &players },
                    );
                }
            }
        }
    }

    /// Send one seat the whole view again, and no one else anything. (In the lobby there is nothing to resend:
    /// a lobby message is always whole.)
    fn resend_game(&mut self, me: PlayerId) {
        let connected = self.connected();
        let Some(g) = &self.game else { return };
        let road_lens = g.longest_roads();
        self.seats[me].synced = None;
        update_seat(&mut self.seats[me], g.view(me, &connected, &road_lens));
    }

    fn handle(&mut self, tx: &Tx, token: &mut Option<String>, msg: ClientMsg) {
        match msg {
            ClientMsg::Control(Control::Join { name, color, token: old, delta }) => {
                if token.is_some() {
                    return;
                }
                // Reconnect to an existing seat? (Whatever it was sent before went to the old connection, so
                // the new one starts from a whole view.)
                if let Some(i) = old.as_deref().and_then(|t| self.seat_of(t)) {
                    self.seats[i].tx = Some(tx.clone());
                    self.seats[i].delta = delta;
                    self.seats[i].synced = None;
                    *token = Some(self.seats[i].token.clone());
                    send(tx, &ServerMsg::Welcome { token: &self.seats[i].token });
                    self.broadcast();
                    return;
                }
                if self.game.is_some() {
                    return error(tx, "A game is already in progress");
                }
                if self.seats.len() >= MAX_PLAYERS {
                    return error(tx, format!("The table is full ({MAX_PLAYERS} players)"));
                }
                // Use the requested color if it is on the palette and free. With no request
                // (older clients) take the first free one; a bad or taken request is refused.
                let color = match color.as_deref() {
                    None => PALETTE.iter().copied().find(|c| !self.color_taken(c)),
                    Some(c) => match palette_color(c) {
                        Some(c) if !self.color_taken(c) => Some(c),
                        Some(_) => {
                            return send(
                                tx,
                                &ServerMsg::Error {
                                    message: "That color was just taken. Pick another.".into(),
                                    code: Some("color_taken"),
                                },
                            )
                        }
                        None => return error(tx, "Unknown color"),
                    },
                };
                let Some(color) = color else {
                    return error(tx, "No colors left");
                };
                let mut name: String = name.trim().chars().take(16).collect();
                if name.is_empty() {
                    name = format!("Player {}", self.seats.len() + 1);
                }
                let t: String = (0..16)
                    .map(|_| char::from_digit(rand::thread_rng().gen_range(0..16), 16).unwrap())
                    .collect();
                self.seats.push(Seat {
                    name,
                    color,
                    token: t.clone(),
                    tx: Some(tx.clone()),
                    delta,
                    synced: None,
                    bot: false,
                });
                send(tx, &ServerMsg::Welcome { token: &t });
                *token = Some(t);
                self.broadcast();
            }
            ClientMsg::Control(Control::Resync) => {
                let Some(me) = token.as_deref().and_then(|t| self.seat_of(t)) else {
                    return error(tx, "Join the table first");
                };
                self.resend_game(me);
            }
            ClientMsg::Control(Control::Start) => {
                let Some(me) = token.as_deref().and_then(|t| self.seat_of(t)) else {
                    return error(tx, "Join the table first");
                };
                if me != HOST {
                    return error(tx, "Only the host can start the game");
                }
                if self.game.is_some() {
                    return error(tx, "The game has already started");
                }
                if self.seats.len() < 2 {
                    return error(tx, "You need at least 2 players");
                }
                if let Err(e) = self.settings.check_players(self.seats.len()) {
                    return error(tx, e);
                }
                let names = self.seats.iter().map(|s| s.name.clone()).collect();
                let colors = self.seats.iter().map(|s| s.color.to_string()).collect();
                // Random draft/turn order, with the timer mode, rules and map the host picked.
                // A map that can't be built (or isn't for this many players) leaves the table in the
                // lobby, and only the host who pressed Start is told why.
                let Some(def) = maps::find(&self.maps, &self.map) else {
                    return error(tx, "Can't start: the chosen map is no longer available. Pick another.");
                };
                match Game::try_with_def(names, colors, self.timer, self.settings, &self.map, &def) {
                    Ok(game) => {
                        self.game = Some(game);
                        // The bots' first move comes a moment after the game appears on everyone's screen.
                        self.bots = bot::Driver::new(Instant::now(), &mut rand::thread_rng());
                        self.broadcast();
                    }
                    Err(e) => return error(tx, format!("Can't start on this map: {e}")),
                }
            }
            ClientMsg::Control(Control::SetTimer { mode }) => {
                let Some(me) = token.as_deref().and_then(|t| self.seat_of(t)) else {
                    return error(tx, "Join the table first");
                };
                if me != HOST {
                    return error(tx, "Only the host can change the timer");
                }
                if self.game.is_some() {
                    return error(tx, "The game has already started");
                }
                self.timer = mode;
                self.broadcast();
            }
            ClientMsg::Control(Control::SetMap { map }) => {
                let Some(me) = token.as_deref().and_then(|t| self.seat_of(t)) else {
                    return error(tx, "Join the table first");
                };
                if me != HOST {
                    return error(tx, "Only the host can change the map");
                }
                if self.game.is_some() {
                    return error(tx, "The game has already started");
                }
                if maps::find(&self.maps, &map).is_none() {
                    // Say what was asked for, but not all of an enormous id.
                    let asked: String = map.chars().take(40).collect();
                    return error(tx, format!("There is no map called `{asked}`"));
                }
                self.map = map;
                self.broadcast();
            }
            ClientMsg::Control(Control::AddBot) => {
                let Some(me) = token.as_deref().and_then(|t| self.seat_of(t)) else {
                    return error(tx, "Join the table first");
                };
                if me != HOST {
                    return error(tx, "Only the host can add a bot");
                }
                if self.game.is_some() {
                    return error(tx, "The game has already started");
                }
                // Bots sit at the same table as everyone, so they count toward the limit.
                if self.seats.len() >= MAX_PLAYERS {
                    return error(tx, format!("The table is full ({MAX_PLAYERS} players, bots included)"));
                }
                let Some(color) = PALETTE.iter().copied().find(|c| !self.color_taken(c)) else {
                    return error(tx, "No colors left");
                };
                let name = bot::bot_name(self.seats.iter().map(|s| s.name.as_str()), &mut rand::thread_rng());
                // Nobody is ever given this token; it only has to be unlike anyone else's.
                let t: String = (0..16)
                    .map(|_| char::from_digit(rand::thread_rng().gen_range(0..16), 16).unwrap())
                    .collect();
                self.seats.push(Seat { name, color, token: t, tx: None, delta: false, synced: None, bot: true });
                self.broadcast();
            }
            ClientMsg::Control(Control::RemoveBot { seat }) => {
                let Some(me) = token.as_deref().and_then(|t| self.seat_of(t)) else {
                    return error(tx, "Join the table first");
                };
                if me != HOST {
                    return error(tx, "Only the host can remove a bot");
                }
                if self.game.is_some() {
                    return error(tx, "The game has already started");
                }
                let at = PlayerId::new(seat);
                if !self.seats.get(at).map_or(false, |s| s.bot) {
                    return error(tx, "There is no bot in that seat");
                }
                self.seats.remove(at);
                self.broadcast();
            }
            ClientMsg::Control(Control::SetSettings { settings }) => {
                let Some(me) = token.as_deref().and_then(|t| self.seat_of(t)) else {
                    return error(tx, "Join the table first");
                };
                if me != HOST {
                    return error(tx, "Only the host can change the game settings");
                }
                if self.game.is_some() {
                    return error(tx, "The game has already started");
                }
                if let Err(e) = settings.validate() {
                    // Tell everyone the real values again so the host's form snaps back.
                    error(tx, e);
                    return self.broadcast();
                }
                self.settings = settings;
                self.broadcast();
            }
            ClientMsg::Control(Control::Reset) => {
                let finished = matches!(&self.game, Some(g) if g.phase == Phase::GameOver);
                if !finished {
                    return error(tx, "The game isn't over yet");
                }
                self.game = None;
                // People who left during the game lose their seats; the bots stay for the next one.
                self.seats.retain(|s| s.tx.is_some() || s.bot);
                self.humans_first();
                self.broadcast();
            }
            ClientMsg::Action(action) => {
                let Some(me) = token.as_deref().and_then(|t| self.seat_of(t)) else {
                    return error(tx, "Join the table first");
                };
                let Some(g) = self.game.as_mut() else {
                    return error(tx, "The game hasn't started yet");
                };
                match g.apply(me, action) {
                    Ok(()) => self.broadcast(),
                    Err(e) => error(tx, e),
                }
            }
        }
    }

    /// Fire any countdown that has run out, let a bot move if one has something to do, and tell everyone if the
    /// game changed.
    fn tick(&mut self) {
        self.tick_at(Instant::now());
    }

    /// `tick` at a given moment (so tests can make time pass).
    fn tick_at(&mut self, now: Instant) {
        let Some(g) = self.game.as_mut() else { return };
        let mut changed = g.tick(now);
        if self.seats.iter().any(|s| s.bot) {
            let is_bot: Vec<bool> = self.seats.iter().map(|s| s.bot).collect();
            changed |= self.bots.step(g, |p| is_bot[p.index()], now, &mut rand::thread_rng());
        }
        if changed {
            self.broadcast();
        }
    }

    fn disconnect(&mut self, token: &str, tx: &Tx) {
        let Some(i) = self.seat_of(token) else { return };
        // Ignore if this seat has already been taken over by a newer connection.
        match &self.seats[i].tx {
            Some(cur) if cur.same_channel(tx) => {}
            _ => return,
        }
        if self.game.is_some() {
            self.seats[i].tx = None;
        } else {
            self.seats.remove(i);
        }
        if self.seats.iter().all(|s| s.tx.is_none()) {
            // Everyone left: forget the game entirely.
            self.seats.clear();
            self.game = None;
            self.timer = TimerMode::default();
            self.settings = Settings::default();
            self.map = builtin::DEFAULT_MAP.to_string();
        } else {
            // If the host was the one to go, the next person is host now (see `humans_first`).
            self.humans_first();
            self.broadcast();
        }
    }
}

/// Which colors are already taken, so the join screen can grey them out before connecting.
#[derive(Serialize)]
struct LobbyInfo {
    taken: Vec<&'static str>,
}

async fn lobby_info(State(room): State<Shared>) -> axum::Json<LobbyInfo> {
    let room = room.lock().unwrap();
    axum::Json(LobbyInfo { taken: room.seats.iter().map(|s| s.color).collect() })
}

/// The most bytes `POST /api/maps` reads: the size of the biggest map file the server accepts anywhere.
const MAX_UPLOAD_BYTES: usize = map_def::MAX_MAP_BYTES;

/// Every way `POST /api/maps` can fail answers with `{"issues": [..]}`, so a client has one thing to show.
fn refuse(status: StatusCode, issues: Vec<String>) -> Response {
    (status, Json(serde_json::json!({ "issues": issues }))).into_response()
}

/// The token in `Authorization: Bearer <token>`.
fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if scheme.eq_ignore_ascii_case("bearer") {
        Some(token.trim())
    } else {
        None
    }
}

/// `POST /api/maps`: the host uploads a map (the body is the map's JSON, at most `MAX_UPLOAD_BYTES`).
///
/// The host is the seat holder `seat_of` finds for the token in `Authorization: Bearer <token>` (the token the
/// server gave them in `welcome`), and it has to be seat 0, as it does for every other host-only message.
/// A good map is added to the lobby's list at once (under a number if its name or id is taken) and answered
/// with `201` and `{"id", "name", "warnings"}`; the lobby is sent to everyone again so the new map shows up.
/// Anything else is answered with `{"issues": [..]}` and a status: 401 no known token, 403 not the host,
/// 413 too big, 422 not a good map (every issue is listed), 409 the upload limit is reached.
/// Uploaded maps are kept in memory only.
async fn upload_map(State(room): State<Shared>, headers: HeaderMap, body: Result<Bytes, BytesRejection>) -> Response {
    // Who is asking? The room's lock is held for this lookup and let go before anything slow happens.
    let maps = {
        let room = room.lock().unwrap();
        match bearer_token(&headers).and_then(|token| room.seat_of(token)) {
            None => return refuse(StatusCode::UNAUTHORIZED, vec!["Send the host's seat token as `Authorization: Bearer <token>`".to_string()]),
            Some(HOST) => {}
            Some(_) => return refuse(StatusCode::FORBIDDEN, vec!["Only the host can upload a map".to_string()]),
        }
        room.maps.clone()
    };

    let bytes = match body {
        Ok(bytes) => bytes,
        Err(rejection) => {
            // `DefaultBodyLimit` makes an oversized body one of these (413).
            let status = rejection.into_response().status();
            let why = if status == StatusCode::PAYLOAD_TOO_LARGE {
                format!("The map is too big: the limit is {MAX_UPLOAD_BYTES} bytes")
            } else {
                "The request body couldn't be read".to_string()
            };
            return refuse(status, vec![why]);
        }
    };
    if maps::uploads_full(&maps) {
        return refuse(StatusCode::CONFLICT, vec![maps::RegistryError::TooManyUploads.to_string()]);
    }
    let Ok(text) = String::from_utf8(bytes.to_vec()) else {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, vec!["The map is not UTF-8 text".to_string()]);
    };

    // Checking a map can take a moment (it deals trial boards), so it happens off the async threads that
    // serve everyone else.
    let checked = tokio::task::spawn_blocking(move || map_loader::check_and_add(&maps, &text, maps::MapSource::Upload, None)).await;
    let added = match checked {
        Ok(Ok(added)) => added,
        Ok(Err(map_loader::AddError::Invalid(issues))) => return refuse(StatusCode::UNPROCESSABLE_ENTITY, issues),
        Ok(Err(map_loader::AddError::Full(why))) => return refuse(StatusCode::CONFLICT, vec![why]),
        Ok(Err(map_loader::AddError::Internal(why))) => return refuse(StatusCode::INTERNAL_SERVER_ERROR, vec![why]),
        Err(_) => return refuse(StatusCode::INTERNAL_SERVER_ERROR, vec!["Something went wrong while checking the map".to_string()]),
    };

    // Everyone in the lobby sees the new map. (A game in progress doesn't list maps, so there is nothing to say.)
    {
        let mut room = room.lock().unwrap();
        if room.game.is_none() {
            room.broadcast();
        }
    }
    (StatusCode::CREATED, Json(serde_json::json!({ "id": added.id, "name": added.name, "warnings": added.warnings }))).into_response()
}

async fn ws_handler(ws: WebSocketUpgrade, State(room): State<Shared>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, room))
}

async fn handle_socket(socket: WebSocket, room: Shared) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = unbounded_channel::<String>();
    let writer = tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            if sink.send(Message::Text(m)).await.is_err() {
                break;
            }
        }
    });

    let mut token: Option<String> = None;
    while let Some(Ok(msg)) = stream.next().await {
        match msg {
            Message::Text(text) => match serde_json::from_str::<ClientMsg>(&text) {
                Ok(m) => room.lock().unwrap().handle(&tx, &mut token, m),
                Err(_) => error(&tx, "Unrecognised message"),
            },
            Message::Close(_) => break,
            _ => {}
        }
    }
    if let Some(t) = token {
        room.lock().unwrap().disconnect(&t, &tx);
    }
    writer.abort();
}

/// Best-effort LAN address, found without sending any packets.
fn lan_ip() -> Option<String> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    Some(s.local_addr().ok()?.ip().to_string())
}

/// Every route the server has, serving the frontend from `static_dir` for anything else.
fn router(room: Shared, static_dir: &str) -> Router {
    Router::new()
        .route("/ws", get(ws_handler))
        .route("/api/lobby", get(lobby_info))
        // A map is small, so this route takes far less than axum's 2 MB default.
        .route("/api/maps", post(upload_map).layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES)))
        .fallback_service(ServeDir::new(static_dir).fallback(ServeFile::new(format!("{static_dir}/index.html"))))
        .with_state(room)
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("validate") {
        std::process::exit(validate::run_cli(&args[1..], &mut std::io::stdout(), &mut std::io::stderr()));
    }

    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3000);
    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "web/dist".to_string());

    // Built-in maps first, then every good `*.json` in the maps folder. A bad file is logged and skipped.
    let maps_dir = std::env::var("MAPS_DIR").unwrap_or_else(|_| map_loader::DEFAULT_MAPS_DIR.to_string());
    let maps = maps::shared_with_builtins();
    let folder = map_loader::load_dir(std::path::Path::new(&maps_dir), &maps);
    let folder_summary = folder.summary(std::path::Path::new(&maps_dir));
    let room: Shared = Arc::new(Mutex::new(Room { maps: maps.clone(), ..Room::default() }));
    let app = router(room.clone(), &static_dir);

    // Turn timers are enforced here, not in the browser, so a closed tab or a
    // disconnected player is auto-played when their time runs out.
    tokio::spawn(async move {
        let mut every = tokio::time::interval(Duration::from_millis(200));
        loop {
            every.tick().await;
            room.lock().unwrap().tick();
        }
    });

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .unwrap_or_else(|e| panic!("could not listen on port {port}: {e}"));

    println!("Settoria LAN server running");
    println!("  Maps:           {}", maps::ids(&maps).join(", "));
    println!("  Maps folder:    {maps_dir}: {folder_summary}");
    println!("  This computer:  http://localhost:{port}");
    // Inside a container the address found by `lan_ip` is the container's own, so `ADVERTISE_HOST` can name the real one.
    let advertised = std::env::var("ADVERTISE_HOST").ok().map(|h| h.trim().to_string()).filter(|h| !h.is_empty());
    if let Some(ip) = advertised.or_else(lan_ip) {
        println!("  Other devices:  http://{ip}:{port}");
    }
    if !std::path::Path::new(&static_dir).join("index.html").exists() {
        println!("  (!) No frontend found in '{static_dir}'. Run: cd web && npm install && npm run build");
    }
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map_def::{MapDef, PlayerRange};
    use crate::maps::MapSource;
    use serde_json::{json, Value};
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

    /// A browser at the table: what the server sends it, and its seat token. Messages go through
    /// `Room::handle` exactly as they do from a socket, parsed from JSON the way the socket parses them.
    struct Client {
        tx: Tx,
        rx: UnboundedReceiver<String>,
        token: Option<String>,
    }

    impl Client {
        fn new() -> Client {
            let (tx, rx) = unbounded_channel();
            Client { tx, rx, token: None }
        }

        fn say(&mut self, room: &mut Room, msg: Value) {
            let msg: ClientMsg = serde_json::from_str(&msg.to_string()).expect("a message the server understands");
            room.handle(&self.tx, &mut self.token, msg);
        }

        /// Everything the server has sent since the last time, oldest first.
        fn heard(&mut self) -> Vec<Value> {
            let mut out = vec![];
            while let Ok(m) = self.rx.try_recv() {
                out.push(serde_json::from_str(&m).expect("the server sends JSON"));
            }
            out
        }

        /// The error messages the server has sent since the last time.
        fn errors(&mut self) -> Vec<String> {
            self.heard().iter().filter(|m| m["type"] == "error").map(|m| m["message"].as_str().unwrap().to_string()).collect()
        }

        /// The most recent message of this type since the last time.
        fn latest(&mut self, kind: &str) -> Option<Value> {
            self.heard().into_iter().rev().find(|m| m["type"] == kind)
        }
    }

    /// A room with these players seated, the first being the host. Whatever they heard while joining is dropped.
    fn table(names: &[&str]) -> (Room, Vec<Client>) {
        let mut room = Room::default();
        let mut clients = vec![];
        for name in names {
            let mut c = Client::new();
            c.say(&mut room, json!({ "type": "join", "name": name }));
            clients.push(c);
        }
        for c in &mut clients {
            c.heard();
        }
        (room, clients)
    }

    fn add_map(room: &Room, id: &str, def: MapDef) {
        room.maps.write().unwrap().insert(id, MapSource::Upload, def).expect("a fresh id");
    }

    fn trio() -> MapDef {
        let mut def = builtin::flower();
        def.name = "Trio".into();
        def.players = PlayerRange { min: 3, max: 4 };
        def
    }

    fn ids(maps: &Value) -> Vec<&str> {
        maps.as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect()
    }

    #[test]
    fn the_lobby_names_the_chosen_map_and_lists_every_map() {
        let (mut room, mut c) = table(&["Ada", "Bo"]);
        room.broadcast();
        let lobby = c[1].latest("lobby").expect("a lobby message");
        assert_eq!(lobby["map"], "standard");
        assert_eq!(ids(&lobby["maps"]), vec!["standard", "earth", "usa"]);
        let earth = &lobby["maps"][1];
        assert_eq!(earth["name"], "Earth");
        assert_eq!((earth["hexes"].as_u64(), earth["players_min"].as_u64(), earth["players_max"].as_u64()), (Some(81), Some(2), Some(8)));
        assert_eq!(earth["builtin"], true);
        assert!(earth.get("recommended_settings").is_none());
        let usa = &lobby["maps"][2];
        assert_eq!(usa["name"], "USA");
        assert_eq!((usa["hexes"].as_u64(), usa["players_min"].as_u64(), usa["players_max"].as_u64()), (Some(144), Some(2), Some(8)));
        assert_eq!(usa["builtin"], true);
    }

    #[test]
    fn the_host_can_choose_any_map_the_server_has_and_everyone_sees_it() {
        let (mut room, mut c) = table(&["Ada", "Bo"]);
        add_map(&room, "user:trio", trio());
        c[0].say(&mut room, json!({ "type": "set_map", "map": "user:trio" }));
        for client in &mut c {
            let lobby = client.latest("lobby").expect("everyone is told");
            assert_eq!(lobby["map"], "user:trio");
            assert_eq!(ids(&lobby["maps"]), vec!["standard", "earth", "usa", "user:trio"], "a map added later is listed too");
            let trio = &lobby["maps"][3];
            assert_eq!((trio["name"].as_str(), trio["builtin"].as_bool(), trio["players_min"].as_u64()), (Some("Trio"), Some(false), Some(3)));
        }
        // and a built-in one again
        c[0].say(&mut room, json!({ "type": "set_map", "map": "earth" }));
        assert_eq!(c[1].latest("lobby").unwrap()["map"], "earth");
    }

    #[test]
    fn an_unknown_map_is_refused_with_a_message_and_nothing_changes() {
        let (mut room, mut c) = table(&["Ada", "Bo"]);
        for bad in ["atlantis", "Earth", "", " earth", "user:trio", "../maps/earth"] {
            c[0].say(&mut room, json!({ "type": "set_map", "map": bad }));
            let errors = c[0].errors();
            assert_eq!(errors.len(), 1, "{bad:?}: {errors:?}");
            assert!(errors[0].contains("There is no map called"), "{bad:?}: {errors:?}");
            assert_eq!(room.map, "standard", "{bad:?}");
            assert!(c[1].heard().is_empty(), "{bad:?}: the others needn't be told about a refused request");
        }
        // an enormous id isn't echoed back in full
        c[0].say(&mut room, json!({ "type": "set_map", "map": "x".repeat(100_000) }));
        let errors = c[0].errors();
        assert!(errors.len() == 1 && errors[0].len() < 120, "{}", errors[0].len());
    }

    #[test]
    fn only_the_host_chooses_the_map_and_only_before_the_game_starts() {
        let (mut room, mut c) = table(&["Ada", "Bo"]);
        // a guest is told off for the host's job whether or not the map exists (so it can't be used to probe)
        for map in ["earth", "atlantis"] {
            c[1].say(&mut room, json!({ "type": "set_map", "map": map }));
            let errors = c[1].errors();
            assert!(errors.len() == 1 && errors[0].contains("host"), "{map}: {errors:?}");
        }
        assert_eq!(room.map, "standard");
        // someone who hasn't joined at all
        let mut stranger = Client::new();
        stranger.say(&mut room, json!({ "type": "set_map", "map": "earth" }));
        assert!(stranger.errors()[0].contains("Join the table first"));

        c[0].say(&mut room, json!({ "type": "start" }));
        assert!(room.game.is_some());
        c[0].heard();
        c[0].say(&mut room, json!({ "type": "set_map", "map": "earth" }));
        assert!(c[0].errors()[0].contains("already started"));
        assert_eq!(room.map, "standard");
    }

    #[test]
    fn eight_players_can_sit_and_a_ninth_is_turned_away() {
        let (mut room, mut c) = table(&["Ada", "Bo", "Cy", "Di", "Ed", "Flo", "Gus", "Hal"]);
        assert_eq!(room.seats.len(), 8);
        let colors: std::collections::HashSet<_> = room.seats.iter().map(|s| s.color).collect();
        assert_eq!(colors.len(), 8, "everyone gets a color of their own");

        let mut ninth = Client::new();
        ninth.say(&mut room, json!({ "type": "join", "name": "Late" }));
        let errors = ninth.errors();
        assert!(errors.len() == 1 && errors[0].contains("table is full") && errors[0].contains('8'), "{errors:?}");
        assert_eq!(room.seats.len(), 8);

        // Standard is a 2 to 4 player board, so the host has to choose a bigger map first
        c[0].say(&mut room, json!({ "type": "start" }));
        let errors = c[0].errors();
        assert!(errors.len() == 1 && errors[0].contains("for 2 to 4 players") && errors[0].contains("8 are seated"), "{errors:?}");
        assert!(room.game.is_none(), "still in the lobby");

        c[0].say(&mut room, json!({ "type": "set_map", "map": "earth" }));
        c[0].heard();
        c[0].say(&mut room, json!({ "type": "start" }));
        assert!(room.game.is_some(), "started with 8 on Earth");
        let view = c[0].latest("game").expect("a game message")["view"].clone();
        assert_eq!(view["players"].as_array().unwrap().len(), 8);
        assert_eq!(view["order"].as_array().unwrap().len(), 8);
        assert_eq!(view["bank"], json!([30, 30, 30, 30, 30]), "the bank grows with the table");
        assert_eq!(view["deck_left"], 44);
    }

    #[test]
    fn a_map_is_only_started_for_the_players_it_is_meant_for() {
        let (mut room, mut c) = table(&["Ada", "Bo"]);
        add_map(&room, "user:trio", trio());
        c[0].say(&mut room, json!({ "type": "set_map", "map": "user:trio" }));
        c[0].heard();
        c[1].heard();

        c[0].say(&mut room, json!({ "type": "start" }));
        let errors = c[0].errors();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("for 3 to 4 players") && errors[0].contains("2 are seated"), "{}", errors[0]);
        assert!(room.game.is_none(), "still in the lobby");
        assert_eq!(room.seats.len(), 2);
        assert!(c[1].heard().is_empty(), "only the host who pressed Start is told");

        // a third player makes it playable
        let mut third = Client::new();
        third.say(&mut room, json!({ "type": "join", "name": "Cy" }));
        c[0].heard();
        c[0].say(&mut room, json!({ "type": "start" }));
        assert!(room.game.is_some(), "started with 3");
        let view = c[0].latest("game").expect("a game message")["view"].clone();
        assert_eq!(view["board"]["map"], "user:trio");
        assert_eq!(view["board"]["hexes"].as_array().unwrap().len(), 7);
        assert!(view["log"].as_array().unwrap().iter().any(|e| e == &json!({ "type": "map_chosen", "map": "user:trio", "name": "Trio" })));
    }

    #[test]
    fn a_suggested_rule_set_is_offered_to_everyone_and_applied_to_no_one() {
        let (mut room, mut c) = table(&["Ada", "Bo"]);
        let mut def = builtin::flower();
        def.name = "Quick".into();
        let mut suggested = Settings::default();
        suggested.points_to_win = 8;
        suggested.friendly_robber = true;
        def.recommended_settings = Some(suggested);
        add_map(&room, "user:quick", def);

        c[0].say(&mut room, json!({ "type": "set_map", "map": "user:quick" }));
        let lobby = c[1].latest("lobby").unwrap();
        assert_eq!(lobby["settings"]["points_to_win"], 15, "choosing the map did not change the rules");
        let offered = &lobby["maps"][3]["recommended_settings"];
        assert_eq!((offered["points_to_win"].as_u64(), offered["friendly_robber"].as_bool()), (Some(8), Some(true)));
        assert!(lobby["maps"][0].get("recommended_settings").is_none());

        c[0].heard();
        c[0].say(&mut room, json!({ "type": "start" }));
        let view = c[0].latest("game").unwrap()["view"].clone();
        assert_eq!(view["settings"]["points_to_win"], 15, "the game is played on the host's rules, not the map's");

        // the host can apply them the ordinary way: by sending them as the settings
        let (mut room, mut c) = table(&["Ada", "Bo"]);
        c[0].say(&mut room, json!({ "type": "set_settings", "settings": serde_json::to_value(suggested).unwrap() }));
        assert_eq!(c[1].latest("lobby").unwrap()["settings"]["points_to_win"], 8);
    }

    #[test]
    fn a_game_on_the_standard_map_does_not_announce_it_and_one_on_earth_does() {
        let (mut room, mut c) = table(&["Ada", "Bo"]);
        c[0].say(&mut room, json!({ "type": "start" }));
        let view = c[0].latest("game").unwrap()["view"].clone();
        assert_eq!(view["board"]["map"], "standard");
        assert!(view["board"].get("kind").is_none(), "the old field is gone");
        assert!(!view["log"].as_array().unwrap().iter().any(|e| e["type"] == "map_chosen"));

        let (mut room, mut c) = table(&["Ada", "Bo"]);
        c[0].say(&mut room, json!({ "type": "set_map", "map": "earth" }));
        c[0].say(&mut room, json!({ "type": "start" }));
        let view = c[0].latest("game").unwrap()["view"].clone();
        assert_eq!(view["board"]["map"], "earth");
        assert_eq!(view["board"]["hexes"].as_array().unwrap().len(), 81);
        assert!(view["log"].as_array().unwrap().iter().any(|e| e == &json!({ "type": "map_chosen", "map": "earth", "name": "Earth" })));
    }

    #[test]
    fn the_table_forgets_its_map_when_everyone_leaves() {
        let (mut room, mut c) = table(&["Ada", "Bo"]);
        c[0].say(&mut room, json!({ "type": "set_map", "map": "earth" }));
        assert_eq!(room.map, "earth");
        for client in &mut c {
            let token = client.token.clone().unwrap();
            room.disconnect(&token, &client.tx);
        }
        assert!(room.seats.is_empty());
        assert_eq!(room.map, "standard");
    }

    #[test]
    fn a_rematch_keeps_the_table_on_the_map_it_chose() {
        let (mut room, mut c) = table(&["Ada", "Bo"]);
        c[0].say(&mut room, json!({ "type": "set_map", "map": "earth" }));
        c[0].say(&mut room, json!({ "type": "start" }));
        assert!(room.game.is_some());
        // end the game the way a finished one looks, then ask for a rematch
        room.game.as_mut().unwrap().phase = Phase::GameOver;
        c[0].heard();
        c[0].say(&mut room, json!({ "type": "reset" }));
        assert!(room.game.is_none());
        let lobby = c[1].latest("lobby").unwrap();
        assert_eq!(lobby["map"], "earth");
    }

    #[test]
    fn the_old_map_names_still_work_as_ids_on_the_wire() {
        // A browser that predates the map list sends the same strings it always did.
        let (mut room, mut c) = table(&["Ada", "Bo"]);
        for map in ["earth", "standard"] {
            c[0].say(&mut room, json!({ "type": "set_map", "map": map }));
            assert_eq!(room.map, map);
        }
    }

    #[test]
    fn a_map_that_vanishes_before_start_is_a_message_not_a_crash() {
        // (nothing removes maps yet; this is the guard for when something does)
        let (mut room, mut c) = table(&["Ada", "Bo"]);
        room.map = "gone".into();
        c[0].say(&mut room, json!({ "type": "start" }));
        assert!(c[0].errors()[0].contains("no longer available"));
        assert!(room.game.is_none());
    }

    // ---------------- POST /api/maps ----------------

    /// A map that is good as it stands: the example in `examples/maps/archipelago.json`.
    const GOOD_MAP: &str = include_str!("../examples/maps/archipelago.json");

    /// The example map with its name changed.
    fn good_map_named(name: &str) -> String {
        let mut v: Value = serde_json::from_str(GOOD_MAP).unwrap();
        v["name"] = json!(name);
        v.to_string()
    }

    fn shared(room: Room) -> Shared {
        Arc::new(Mutex::new(room))
    }

    fn bearer(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, format!("Bearer {token}").parse().unwrap());
        headers
    }

    /// Call the upload handler the way the router does, and read its answer.
    async fn post_map(room: &Shared, headers: HeaderMap, body: impl Into<Bytes>) -> (StatusCode, Value) {
        let response = upload_map(State(room.clone()), headers, Ok(body.into())).await;
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).expect("every answer is JSON"))
    }

    fn map_ids(room: &Shared) -> Vec<String> {
        let maps = room.lock().unwrap().maps.clone();
        maps::ids(&maps)
    }

    #[test]
    fn the_router_can_be_built() {
        // axum panics when a router is put together wrongly (overlapping routes, say), so build it once
        let _ = router(shared(Room::default()), "web/dist");
    }

    #[test]
    fn the_upload_limit_is_the_size_of_a_map_file() {
        assert_eq!(MAX_UPLOAD_BYTES, 256 * 1024);
    }

    #[tokio::test]
    async fn the_host_can_upload_a_map_and_the_lobby_shows_it_at_once() {
        let (room, mut c) = table(&["Ada", "Bo"]);
        let tokens: Vec<String> = c.iter().map(|c| c.token.clone().unwrap()).collect();
        let room = shared(room);

        let (status, body) = post_map(&room, bearer(&tokens[0]), GOOD_MAP).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!((body["id"].as_str(), body["name"].as_str()), (Some("archipelago"), Some("Archipelago")));
        assert!(body["warnings"].is_array());
        assert_eq!(map_ids(&room), vec!["standard", "earth", "usa", "archipelago"]);

        // everyone at the table is sent the lobby again, with the new map in its list
        for client in &mut c {
            let lobby = client.latest("lobby").expect("the lobby is sent again");
            assert_eq!(ids(&lobby["maps"]), vec!["standard", "earth", "usa", "archipelago"]);
            assert_eq!(lobby["maps"][3]["builtin"], false);
        }
    }

    #[tokio::test]
    async fn an_uploaded_map_can_be_chosen_and_played() {
        let (room, mut c) = table(&["Ada", "Bo"]);
        let token = c[0].token.clone().unwrap();
        let room = shared(room);
        let (status, _) = post_map(&room, bearer(&token), GOOD_MAP).await;
        assert_eq!(status, StatusCode::CREATED);
        {
            let mut guard = room.lock().unwrap();
            c[0].say(&mut guard, json!({ "type": "set_map", "map": "archipelago" }));
            c[0].say(&mut guard, json!({ "type": "start" }));
            assert!(guard.game.is_some(), "{:?}", c[0].errors());
        }
        let view = c[0].latest("game").expect("a game message")["view"].clone();
        assert_eq!(view["board"]["map"], "archipelago");
        assert_eq!(view["board"]["hexes"].as_array().unwrap().len(), 14);
    }

    #[tokio::test]
    async fn only_the_host_can_upload() {
        let (room, c) = table(&["Ada", "Bo"]);
        let tokens: Vec<String> = c.iter().map(|c| c.token.clone().unwrap()).collect();
        let room = shared(room);

        // a guest: known, but not the host
        let (status, body) = post_map(&room, bearer(&tokens[1]), GOOD_MAP).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(body["issues"][0].as_str().unwrap().contains("host"), "{body}");

        // nobody, or a token the server never gave out, or the wrong kind of header
        let mut basic = HeaderMap::new();
        basic.insert(header::AUTHORIZATION, format!("Basic {}", tokens[0]).parse().unwrap());
        let mut bare = HeaderMap::new();
        bare.insert(header::AUTHORIZATION, tokens[0].parse().unwrap());
        for headers in [HeaderMap::new(), bearer("0000000000000000"), bearer(""), basic, bare] {
            let (status, body) = post_map(&room, headers, GOOD_MAP).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
            assert!(body["issues"].as_array().unwrap().len() == 1);
        }
        assert_eq!(map_ids(&room), vec!["standard", "earth", "usa"], "none of them added anything");

        // the word `Bearer` can be written in any case
        let mut lower = HeaderMap::new();
        lower.insert(header::AUTHORIZATION, format!("bearer {}", tokens[0]).parse().unwrap());
        assert_eq!(post_map(&room, lower, GOOD_MAP).await.0, StatusCode::CREATED);
    }

    #[tokio::test]
    async fn whoever_is_host_now_is_the_host_who_can_upload() {
        // when the host leaves the lobby the next player becomes seat 0, as for every host-only message
        let (mut room, mut c) = table(&["Ada", "Bo"]);
        let tokens: Vec<String> = c.iter().map(|c| c.token.clone().unwrap()).collect();
        room.disconnect(&tokens[0], &c[0].tx);
        c[1].heard();
        let room = shared(room);
        assert_eq!(post_map(&room, bearer(&tokens[0]), GOOD_MAP).await.0, StatusCode::UNAUTHORIZED, "Ada is gone");
        assert_eq!(post_map(&room, bearer(&tokens[1]), GOOD_MAP).await.0, StatusCode::CREATED, "Bo is the host now");
    }

    #[tokio::test]
    async fn a_bad_map_gets_the_list_of_issues_and_adds_nothing() {
        let (room, c) = table(&["Ada"]);
        let token = c[0].token.clone().unwrap();
        let room = shared(room);

        let (status, body) = post_map(&room, bearer(&token), good_map_named(&"n".repeat(100))).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        let issues: Vec<&str> = body["issues"].as_array().unwrap().iter().map(|i| i.as_str().unwrap()).collect();
        assert!(issues.iter().any(|i| i.contains("100 characters")), "{issues:?}");

        // several problems at once are all listed
        let mut v: Value = serde_json::from_str(GOOD_MAP).unwrap();
        v["name"] = json!("two\nlines");
        v["players"] = json!({ "min": 1, "max": 9 });
        let (status, body) = post_map(&room, bearer(&token), v.to_string()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(body["issues"].as_array().unwrap().len() >= 2, "{body}");

        // not a map at all: JSON that doesn't fit, text that isn't JSON, bytes that aren't text
        for junk in [&b"{ nope"[..], b"", b"[]", b"{\"version\":1,\"name\":\"x\",\"hexs\":[]}", &[0xFF, 0xFE, 0x00][..]] {
            let (status, body) = post_map(&room, bearer(&token), junk.to_vec()).await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{junk:?}: {body}");
            assert!(!body["issues"].as_array().unwrap().is_empty(), "{junk:?}");
        }
        assert_eq!(map_ids(&room), vec!["standard", "earth", "usa"]);
    }

    #[tokio::test]
    async fn a_map_over_the_size_limit_is_refused_even_if_the_layer_let_it_through() {
        let (room, c) = table(&["Ada"]);
        let token = c[0].token.clone().unwrap();
        let room = shared(room);
        let big = format!("{{\"version\":1,\"name\":\"x\",\"hexes\":[],\"pad\":\"{}\"}}", "x".repeat(MAX_UPLOAD_BYTES));
        let (status, body) = post_map(&room, bearer(&token), big).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(body["issues"][0].as_str().unwrap().contains("limited to"), "{body}");
    }

    #[tokio::test]
    async fn a_name_or_id_that_is_taken_gets_a_number() {
        let (room, c) = table(&["Ada"]);
        let token = c[0].token.clone().unwrap();
        let room = shared(room);
        let (_, first) = post_map(&room, bearer(&token), GOOD_MAP).await;
        let (status, second) = post_map(&room, bearer(&token), GOOD_MAP).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!((first["id"].as_str(), first["name"].as_str()), (Some("archipelago"), Some("Archipelago")));
        assert_eq!((second["id"].as_str(), second["name"].as_str()), (Some("archipelago-2"), Some("Archipelago (2)")));
        // a map called Earth doesn't replace Earth
        let (status, third) = post_map(&room, bearer(&token), good_map_named("Earth")).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!((third["id"].as_str(), third["name"].as_str()), (Some("earth-2"), Some("Earth (2)")));
        let maps = room.lock().unwrap().maps.clone();
        assert_eq!(maps::find(&maps, "earth").unwrap().hexes.len(), 81);
    }

    #[tokio::test]
    async fn no_more_than_ten_maps_can_be_uploaded() {
        let (room, c) = table(&["Ada"]);
        let token = c[0].token.clone().unwrap();
        let room = shared(room);
        for i in 0..maps::MAX_UPLOADS {
            let (status, body) = post_map(&room, bearer(&token), good_map_named(&format!("Map {i}"))).await;
            assert_eq!(status, StatusCode::CREATED, "{i}: {body}");
        }
        let (status, body) = post_map(&room, bearer(&token), good_map_named("One too many")).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body["issues"][0].as_str().unwrap().contains("10"), "{body}");
        assert_eq!(map_ids(&room).len(), 3 + maps::MAX_UPLOADS);
    }

    #[tokio::test]
    async fn an_upload_during_a_game_is_kept_and_does_not_disturb_it() {
        let (mut room, mut c) = table(&["Ada", "Bo"]);
        let token = c[0].token.clone().unwrap();
        c[0].say(&mut room, json!({ "type": "start" }));
        assert!(room.game.is_some());
        c[0].heard();
        c[1].heard();
        let room = shared(room);
        let (status, _) = post_map(&room, bearer(&token), GOOD_MAP).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(map_ids(&room), vec!["standard", "earth", "usa", "archipelago"]);
        assert!(c[0].heard().is_empty(), "a game doesn't list maps, so nothing is sent");
    }

    // ---- patches -------------------------------------------------------------------------------------

    /// Like `table`, but the players named in `patched` ask for patches when they join.
    fn table_with_patches(names: &[&str], patched: &[&str]) -> (Room, Vec<Client>) {
        let mut room = Room::default();
        let mut clients = vec![];
        for name in names {
            let mut c = Client::new();
            c.say(&mut room, json!({ "type": "join", "name": name, "delta": patched.contains(name) }));
            clients.push(c);
        }
        for c in &mut clients {
            c.heard();
        }
        (room, clients)
    }

    /// What a client that asks for patches does with what it is sent (web/src/patch.ts does the same).
    fn apply_to(view: &mut Value, msg: &Value) {
        match msg["type"].as_str() {
            Some("game") => *view = msg["view"].clone(),
            Some("patch") => {
                patch::apply(view, msg["ops"].as_array().expect("a patch has ops"));
                if let Some(events) = msg["log"].as_array() {
                    let held = view["log_start"].as_u64().unwrap() as usize + view["log"].as_array().unwrap().len();
                    assert_eq!(msg["log_from"].as_u64().unwrap() as usize, held, "the new events follow on from the ones held");
                    let log = view["log"].as_array_mut().unwrap();
                    log.extend(events.iter().cloned());
                    let dropped = log.len().saturating_sub(game::LOG_WINDOW);
                    log.drain(..dropped);
                    let start = view["log_start"].as_u64().unwrap() + dropped as u64;
                    view["log_start"] = json!(start);
                }
            }
            _ => {}
        }
    }

    /// A view with its countdown's `remaining_ms` zeroed: that is the one thing a patched client's copy may
    /// differ in (see `steady_timer`).
    fn without_clock(mut view: Value) -> Value {
        if let Some(clock) = view.get_mut("timer").and_then(Value::as_object_mut) {
            clock.insert("remaining_ms".to_string(), json!(0));
        }
        view
    }

    /// The whole view of this seat as the server would build it now.
    fn truth(room: &Room, seat: usize) -> Value {
        let g = room.game.as_ref().expect("a game");
        serde_json::to_value(g.view(PlayerId::new(seat), &room.connected(), &g.longest_roads())).unwrap()
    }

    /// The next move of whoever's turn it is, for as long as it is one this test can make without looking: placing
    /// the opening pieces, rolling and ending the turn. `None` once a roll leads somewhere else (a 7, a volcano).
    fn next_move(room: &Room) -> Option<(usize, Value)> {
        let g = room.game.as_ref()?;
        let me = g.current;
        let legal = g.legal(me);
        let action = match g.phase {
            Phase::Setup => match legal.settlements.first() {
                Some(&vertex) => json!({ "type": "build_settlement", "vertex": vertex }),
                None => json!({ "type": "build_road", "edge": legal.roads[0] }),
            },
            Phase::Roll => json!({ "type": "roll" }),
            Phase::Main => json!({ "type": "end_turn" }),
            _ => return None,
        };
        Some((me.index(), action))
    }

    /// Start the game and play it with `next_move`. The first `patched` clients ask for patches and the rest
    /// don't. After every move each patched client's copy of its view, kept up with patches alone, must be the
    /// view the server would build, and the others must be sent whole views as always.
    /// Returns the size of the first whole view and of the biggest patch.
    fn play_and_compare(room: &mut Room, c: &mut [Client], patched: usize, max_moves: usize) -> (usize, usize) {
        c[0].say(room, json!({ "type": "start" }));
        assert!(room.game.is_some(), "{:?}", c[0].errors());
        let first: Vec<Value> = c.iter_mut().map(|x| x.latest("game").expect("a game message")).collect();
        for (i, m) in first.iter().enumerate() {
            assert!(m["view"]["board"]["hexes"].is_array(), "seat {i} is sent the board to begin with");
            assert_eq!(m.get("seq").is_some(), i < patched, "only seats that ask for patches are numbered");
            if i < patched {
                assert_eq!(m["seq"], 0);
            }
        }
        let whole = first[0].to_string().len();
        let mut views: Vec<Value> = first[..patched].iter().map(|m| m["view"].clone()).collect();
        let mut biggest = 0;
        let mut moves = 0;
        while let Some((who, action)) = next_move(room) {
            c[who].say(room, action);
            let heard: Vec<Vec<Value>> = c.iter_mut().map(|x| x.heard()).collect();
            for m in heard.iter().flatten() {
                assert_ne!(m["type"], "error", "{m}");
            }
            for (i, messages) in heard.iter().enumerate() {
                for m in messages {
                    if i < patched {
                        if m["type"] == "patch" {
                            biggest = biggest.max(m.to_string().len());
                            for op in m["ops"].as_array().unwrap() {
                                assert_ne!(op[0][0], "board", "the board is never patched");
                            }
                            assert!(m.get("view").is_none());
                        }
                        apply_to(&mut views[i], m);
                    } else {
                        assert_eq!(m["type"], "game");
                        assert!(m["view"]["board"].is_object(), "a seat that doesn't ask for patches is sent the board each time");
                        assert!(m.get("seq").is_none());
                    }
                }
            }
            for i in 0..patched {
                assert_eq!(
                    without_clock(views[i].clone()),
                    without_clock(truth(room, i)),
                    "after move {moves}: seat {i}'s patched view is not the real one"
                );
            }
            moves += 1;
            if moves == max_moves {
                break;
            }
        }
        assert!(moves >= 12, "the opening alone is a dozen moves, only {moves} were played");
        (whole, biggest)
    }

    #[test]
    fn a_patched_seat_always_holds_the_real_view_and_a_seat_that_never_asked_is_sent_whole_ones() {
        let (mut room, mut c) = table_with_patches(&["Ada", "Bo", "Cy"], &["Ada", "Bo"]);
        let (whole, biggest) = play_and_compare(&mut room, &mut c, 2, 40);
        assert!(biggest * 4 < whole, "a patch is at most {biggest} bytes, a whole view {whole}");
    }

    #[test]
    fn patches_are_a_small_fraction_of_a_whole_view_on_a_big_map() {
        let (mut room, mut c) = table_with_patches(&["Ada", "Bo", "Cy", "Di"], &["Ada", "Bo", "Cy", "Di"]);
        c[0].say(&mut room, json!({ "type": "set_map", "map": "earth" }));
        c[0].heard();
        let (whole, biggest) = play_and_compare(&mut room, &mut c, 4, 40);
        assert!(biggest * 10 < whole, "a patch is at most {biggest} bytes, a whole view {whole}");
    }

    #[test]
    fn an_update_with_nothing_new_says_nothing_to_a_patched_seat() {
        let (mut room, mut c) = table_with_patches(&["Ada", "Bo", "Cy"], &["Ada", "Bo"]);
        c[0].say(&mut room, json!({ "type": "start" }));
        for x in &mut c {
            x.heard();
        }
        room.broadcast();
        assert!(c[0].heard().is_empty() && c[1].heard().is_empty(), "nothing changed, so nothing is sent");
        assert_eq!(c[2].heard().len(), 1, "a seat that didn't ask is sent the view regardless");
    }

    #[test]
    fn a_patched_seat_is_sent_the_whole_view_again_when_it_resyncs_reconnects_or_a_new_game_starts() {
        let (mut room, mut c) = table_with_patches(&["Ada", "Bo"], &["Ada", "Bo"]);
        c[0].say(&mut room, json!({ "type": "start" }));
        for x in &mut c {
            x.heard();
        }

        // it asks, and only it is sent anything
        c[0].say(&mut room, json!({ "type": "resync" }));
        let heard = c[0].heard();
        assert_eq!(heard.len(), 1, "{heard:?}");
        assert_eq!(heard[0]["type"], "game");
        assert_eq!(heard[0]["seq"], 0);
        assert!(heard[0]["view"]["board"].is_object());
        assert!(c[1].heard().is_empty());
        // and the patches after it count on from there
        let (who, action) = next_move(&room).unwrap();
        c[who].say(&mut room, action);
        assert_eq!(c[0].latest("patch").expect("a patch")["seq"], 1);

        // it reconnects (a new connection with the seat's token)
        let token = c[0].token.clone().unwrap();
        let mut again = Client::new();
        again.say(&mut room, json!({ "type": "join", "name": "Ada", "token": token, "delta": true }));
        let heard = again.heard();
        let whole = heard.iter().find(|m| m["type"] == "game").expect("a whole view");
        assert_eq!(whole["seq"], 0);
        assert!(whole["view"]["board"].is_object());

        // the game ends and the next one starts
        c[1].heard();
        room.game = None;
        room.broadcast();
        assert!(again.latest("lobby").is_some() && c[1].latest("lobby").is_some());
        again.say(&mut room, json!({ "type": "start" }));
        for client in [&mut again, &mut c[1]] {
            let m = client.latest("game").expect("a game message");
            assert_eq!(m["seq"], 0);
            assert!(m["view"]["board"].is_object(), "a new game is sent with its board");
        }
    }

    #[test]
    fn resync_before_joining_is_refused_and_in_the_lobby_changes_nothing() {
        let mut room = Room::default();
        let mut stranger = Client::new();
        stranger.say(&mut room, json!({ "type": "resync" }));
        assert_eq!(stranger.errors(), vec!["Join the table first".to_string()]);
        let (mut room, mut c) = table_with_patches(&["Ada", "Bo"], &["Ada", "Bo"]);
        c[0].say(&mut room, json!({ "type": "resync" }));
        assert!(c[0].heard().is_empty() && c[1].heard().is_empty());
    }

    #[test]
    fn the_log_goes_out_by_number_and_a_long_game_keeps_the_same_window_as_a_whole_view() {
        let (mut room, mut c) = table_with_patches(&["Ada", "Bo"], &["Ada", "Bo"]);
        c[0].say(&mut room, json!({ "type": "start" }));
        let mut view = c[0].latest("game").unwrap()["view"].clone();
        c[1].heard();
        // a few hundred events, a few at a time, pushes events off the front of the window
        for round in 0..120 {
            let g = room.game.as_mut().unwrap();
            for _ in 0..(1 + round % 4) {
                g.push_log_for_test(LogEvent::LongestRoadLost);
            }
            room.broadcast();
            for m in c[0].heard() {
                apply_to(&mut view, &m);
            }
            c[1].heard();
        }
        let real = truth(&room, 0);
        assert_eq!(view["log"], real["log"]);
        assert_eq!(view["log_start"], real["log_start"]);
        assert_eq!(view["log"].as_array().unwrap().len(), game::LOG_WINDOW);
        assert!(view["log_start"].as_u64().unwrap() > 0, "the window has slid");
    }

    #[test]
    fn a_countdown_that_runs_as_the_client_expects_is_not_sent_again() {
        let clock = |remaining_ms: u64, player: u64| json!({ "timer": { "kind": "turn", "player": player, "remaining_ms": remaining_ms, "total_ms": 90_000 } });
        let sent_at = Instant::now();
        let now = sent_at + Duration::from_millis(10_000);

        // 10 s later it has 80 s left, which is where the client has counted to: not sent, and the baseline stays
        let mut next = clock(80_100, 1);
        assert_eq!(steady_timer(&clock(90_000, 1), &mut next, sent_at, now), sent_at);
        assert_eq!(next, clock(90_000, 1));
        let mut ops = vec![];
        patch::diff(&clock(90_000, 1), &next, &mut ops);
        assert!(ops.is_empty());

        // a bonus put time back on it: sent
        let mut next = clock(85_000, 1);
        assert_eq!(steady_timer(&clock(90_000, 1), &mut next, sent_at, now), now);
        assert_eq!(next, clock(85_000, 1));

        // someone else's clock, or one that started or stopped: sent
        let mut next = clock(80_000, 2);
        assert_eq!(steady_timer(&clock(90_000, 1), &mut next, sent_at, now), now);
        assert_eq!(next, clock(80_000, 2));
        let mut next = json!({ "timer": null });
        assert_eq!(steady_timer(&clock(90_000, 1), &mut next, sent_at, now), now);
        assert_eq!(next, json!({ "timer": null }));
        let mut next = clock(90_000, 1);
        assert_eq!(steady_timer(&json!({ "timer": null }), &mut next, sent_at, now), now);
        assert_eq!(next, clock(90_000, 1));
    }

    // ------------------------------------------------------------------
    // Bots at the table
    // ------------------------------------------------------------------

    fn lobby_names(lobby: &Value) -> Vec<String> {
        lobby["players"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap().to_string()).collect()
    }

    fn seat_names(room: &Room) -> Vec<&str> {
        room.seats.iter().map(|s| s.name.as_str()).collect()
    }

    /// The names of the bots at the table, in seat order.
    fn bot_names(room: &Room) -> Vec<&str> {
        room.seats.iter().filter(|s| s.bot).map(|s| s.name.as_str()).collect()
    }

    /// Whether every one of these names is on the bots' list and none is there twice.
    fn from_the_list_once(names: &[&str]) -> bool {
        let distinct: std::collections::HashSet<&&str> = names.iter().collect();
        names.iter().all(|n| bot::NAMES.contains(n)) && distinct.len() == names.len()
    }

    #[test]
    fn the_host_adds_bots_and_the_lobby_shows_them() {
        let (mut room, mut c) = table(&["Ada", "Bo"]);
        c[0].say(&mut room, json!({ "type": "add_bot" }));
        c[0].say(&mut room, json!({ "type": "add_bot" }));
        assert!(c[0].errors().is_empty());

        let lobby = c[1].latest("lobby").expect("everyone is told");
        let names = lobby_names(&lobby);
        assert_eq!(names[..2], ["Ada", "Bo"]);
        let bots: Vec<&str> = names[2..].iter().map(String::as_str).collect();
        assert_eq!(bots.len(), 2);
        assert!(from_the_list_once(&bots), "bots are named from the list, each once: {bots:?}");
        let flags: Vec<(bool, bool)> =
            lobby["players"].as_array().unwrap().iter().map(|p| (p["bot"].as_bool().unwrap(), p["connected"].as_bool().unwrap())).collect();
        assert_eq!(flags, [(false, true), (false, true), (true, true), (true, true)], "a bot is a bot, and is never away");
        let colors: std::collections::HashSet<&str> = lobby["players"].as_array().unwrap().iter().map(|p| p["color"].as_str().unwrap()).collect();
        assert_eq!(colors.len(), 4, "every bot gets a color of its own");
        assert_eq!(lobby["you"], 1);
        assert_eq!(room.seats.len(), 4);
    }

    #[test]
    fn only_the_host_adds_and_removes_bots_and_only_before_the_game() {
        let (mut room, mut c) = table(&["Ada", "Bo"]);
        c[1].say(&mut room, json!({ "type": "add_bot" }));
        let errors = c[1].errors();
        assert!(errors.len() == 1 && errors[0].contains("host"), "{errors:?}");
        let mut stranger = Client::new();
        stranger.say(&mut room, json!({ "type": "add_bot" }));
        assert!(stranger.errors()[0].contains("Join the table first"));
        assert_eq!(room.seats.len(), 2);

        c[0].say(&mut room, json!({ "type": "add_bot" }));
        c[0].heard();
        c[1].heard();
        c[1].say(&mut room, json!({ "type": "remove_bot", "seat": 2 }));
        let errors = c[1].errors();
        assert!(errors.len() == 1 && errors[0].contains("host"), "{errors:?}");
        stranger.say(&mut room, json!({ "type": "remove_bot", "seat": 2 }));
        assert!(stranger.errors()[0].contains("Join the table first"));
        assert_eq!(room.seats.len(), 3);

        // Only a bot can be taken away: not the host, not another person, not a seat that isn't there.
        for seat in [0, 1, 3, 99] {
            c[0].say(&mut room, json!({ "type": "remove_bot", "seat": seat }));
            let errors = c[0].errors();
            assert!(errors.len() == 1 && errors[0].contains("no bot"), "seat {seat}: {errors:?}");
        }
        assert_eq!(seat_names(&room)[..2], ["Ada", "Bo"]);
        assert_eq!(bot_names(&room).len(), 1);

        c[0].say(&mut room, json!({ "type": "start" }));
        assert!(room.game.is_some(), "two people and a bot make a game");
        c[0].heard();
        c[0].say(&mut room, json!({ "type": "add_bot" }));
        assert!(c[0].errors()[0].contains("already started"));
        c[0].say(&mut room, json!({ "type": "remove_bot", "seat": 2 }));
        assert!(c[0].errors()[0].contains("already started"));
        assert_eq!(room.seats.len(), 3);
    }

    #[test]
    fn bots_count_toward_the_eight_seats_and_come_and_go() {
        let (mut room, mut c) = table(&["Ada", "Bo", "Cy"]);
        for _ in 0..5 {
            c[0].say(&mut room, json!({ "type": "add_bot" }));
        }
        assert!(c[0].errors().is_empty());
        assert_eq!(room.seats.len(), 8);
        assert!(from_the_list_once(&bot_names(&room)));
        let colors: std::collections::HashSet<_> = room.seats.iter().map(|s| s.color).collect();
        assert_eq!(colors.len(), 8, "everyone, bots included, has a color of their own");

        c[0].say(&mut room, json!({ "type": "add_bot" }));
        let errors = c[0].errors();
        assert!(errors.len() == 1 && errors[0].contains("full") && errors[0].contains('8'), "{errors:?}");
        let mut late = Client::new();
        late.say(&mut room, json!({ "type": "join", "name": "Late" }));
        let errors = late.errors();
        assert!(errors.len() == 1 && errors[0].contains("table is full"), "{errors:?}");
        assert_eq!(room.seats.len(), 8);

        // Take a bot away from the middle of the table: the seats after it move up, and another bot can sit down.
        c[0].say(&mut room, json!({ "type": "remove_bot", "seat": 5 }));
        assert_eq!(room.seats.len(), 7);
        assert_eq!(bot_names(&room).len(), 4);
        assert!(from_the_list_once(&bot_names(&room)));
        c[0].say(&mut room, json!({ "type": "add_bot" }));
        assert_eq!(room.seats.len(), 8);
        assert!(from_the_list_once(&bot_names(&room)));
        assert_eq!(c[2].latest("lobby").unwrap()["players"].as_array().unwrap().len(), 8);
    }

    #[test]
    fn a_table_of_bots_uses_every_name_once_and_a_name_that_is_freed_comes_back() {
        let (mut room, mut c) = table(&["Ada"]);
        for _ in 0..7 {
            c[0].say(&mut room, json!({ "type": "add_bot" }));
        }
        assert!(c[0].errors().is_empty());
        assert_eq!(room.seats.len(), 8);
        let mut drawn = bot_names(&room);
        drawn.sort();
        let mut all = bot::NAMES.to_vec();
        all.sort();
        assert_eq!(drawn, all, "seven bots, seven names");

        // Take one away: its name is free again, and it is the only one that is, so the next bot has it.
        let gone = room.seats[PlayerId::new(3)].name.clone();
        c[0].say(&mut room, json!({ "type": "remove_bot", "seat": 3 }));
        assert!(!seat_names(&room).contains(&gone.as_str()));
        c[0].say(&mut room, json!({ "type": "add_bot" }));
        assert_eq!(room.seats[PlayerId::new(7)].name, gone);
    }

    #[test]
    fn a_bot_never_takes_a_name_a_person_has() {
        let (mut room, mut c) = table(&["Ada", "hal 9000"]);
        for _ in 0..6 {
            c[0].say(&mut room, json!({ "type": "add_bot" }));
        }
        assert!(c[0].errors().is_empty());
        assert_eq!(room.seats.len(), 8);
        let bots = bot_names(&room);
        assert!(from_the_list_once(&bots), "{bots:?}");
        assert!(bots.iter().all(|n| !n.eq_ignore_ascii_case("hal 9000")), "{bots:?}");
    }

    #[test]
    fn a_game_with_bots_lists_them_as_present() {
        let (mut room, mut c) = table(&["Ada"]);
        for _ in 0..3 {
            c[0].say(&mut room, json!({ "type": "add_bot" }));
        }
        c[0].say(&mut room, json!({ "type": "start" }));
        assert!(room.game.is_some());
        let view = c[0].latest("game").expect("a game message")["view"].clone();
        let players = view["players"].as_array().unwrap();
        assert_eq!(players.len(), 4);
        assert!(players.iter().all(|p| p["connected"] == true), "{players:?}");
    }

    #[test]
    fn the_host_is_always_a_person() {
        // Seats: Ada (host), a bot, Bo.
        let (mut room, mut c) = table(&["Ada"]);
        c[0].say(&mut room, json!({ "type": "add_bot" }));
        let mut bo = Client::new();
        bo.say(&mut room, json!({ "type": "join", "name": "Bo" }));
        bo.heard();
        let bot = room.seats[PlayerId::new(1)].name.clone();
        assert_eq!(seat_names(&room), ["Ada", bot.as_str(), "Bo"]);

        // The host leaves: Bo, not the bot, is first now.
        room.disconnect(c[0].token.as_deref().unwrap(), &c[0].tx);
        assert_eq!(seat_names(&room), ["Bo", bot.as_str()]);
        let lobby = bo.latest("lobby").expect("Bo is told");
        assert_eq!(lobby["you"], 0);
        assert_eq!(lobby_names(&lobby), ["Bo", bot.as_str()]);

        // And Bo can do the host's things.
        bo.say(&mut room, json!({ "type": "add_bot" }));
        bo.say(&mut room, json!({ "type": "start" }));
        assert!(bo.errors().is_empty());
        assert!(room.game.is_some());
    }

    #[test]
    fn a_table_with_no_people_left_is_forgotten_bots_and_all() {
        let (mut room, mut c) = table(&["Ada"]);
        c[0].say(&mut room, json!({ "type": "add_bot" }));
        c[0].say(&mut room, json!({ "type": "add_bot" }));
        room.disconnect(c[0].token.as_deref().unwrap(), &c[0].tx);
        assert!(room.seats.is_empty());
        assert!(room.game.is_none());

        // The next person to sit down finds an empty table and is the host.
        let mut bo = Client::new();
        bo.say(&mut room, json!({ "type": "join", "name": "Bo" }));
        let lobby = bo.latest("lobby").unwrap();
        assert_eq!((lobby["you"].as_u64(), lobby_names(&lobby)), (Some(0), vec!["Bo".to_string()]));
    }

    #[test]
    fn the_bots_stay_for_the_next_game_and_the_people_who_left_do_not() {
        // Seats: Ada (host), a bot, Bo.
        let (mut room, mut c) = table(&["Ada"]);
        c[0].say(&mut room, json!({ "type": "add_bot" }));
        let mut bo = Client::new();
        bo.say(&mut room, json!({ "type": "join", "name": "Bo" }));
        let bot = room.seats[PlayerId::new(1)].name.clone();
        c[0].say(&mut room, json!({ "type": "start" }));
        assert!(room.game.is_some());

        // The host drops out mid-game: the seat is kept for them, as for any player.
        room.disconnect(c[0].token.as_deref().unwrap(), &c[0].tx);
        assert_eq!(seat_names(&room), ["Ada", bot.as_str(), "Bo"]);

        // The game ends and Bo takes everyone back to the lobby.
        room.game.as_mut().unwrap().phase = Phase::GameOver;
        bo.heard();
        bo.say(&mut room, json!({ "type": "reset" }));
        assert!(room.game.is_none());
        assert_eq!(seat_names(&room), ["Bo", bot.as_str()], "Ada's seat is gone and Bo, a person, is first, and the bot kept its name");
        let lobby = bo.latest("lobby").expect("Bo is back in the lobby");
        assert_eq!(lobby["you"], 0);
        assert_eq!(lobby["players"][1]["bot"], true);
    }

    #[test]
    fn a_bot_plays_its_turns_when_the_clock_ticks() {
        use rand::rngs::StdRng;
        use rand::SeedableRng;

        let (mut room, mut c) = table(&["Ada"]);
        c[0].say(&mut room, json!({ "type": "add_bot" }));
        c[0].say(&mut room, json!({ "type": "start" }));
        assert!(room.game.is_some());
        c[0].heard();

        // Ada plays her own turns (with the bot's brain, for want of a person) and the server clock plays the bot's.
        let bot_seat = PlayerId::new(1);
        let mut rng = StdRng::seed_from_u64(3);
        let mut now = Instant::now();
        let mut acted = 0;
        for _ in 0..600 {
            now += Duration::from_secs(1);
            let g = room.game.as_mut().unwrap();
            if g.phase == Phase::GameOver {
                break;
            }
            if g.current != HOST {
                acted = 0;
            }
            match bot::choose(g, HOST, acted, &mut rng) {
                Some(action) => {
                    let (ends, on_turn) = (matches!(action, Action::EndTurn), g.current == HOST);
                    g.apply_at(HOST, action, now).expect("a legal move");
                    acted = if ends { 0 } else { acted + u32::from(on_turn) };
                    room.broadcast();
                }
                None => room.tick_at(now),
            }
        }
        let g = room.game.as_ref().unwrap();
        let bot_buildings = g.buildings.iter().flatten().filter(|b| b.owner == bot_seat).count();
        assert!(bot_buildings >= 2, "the bot placed its starting settlements");
        assert!(g.roads.iter().flatten().filter(|&&r| r == bot_seat).count() >= 2, "and its starting roads");
        let rolls: u32 = g.roll_counts.iter().sum();
        assert!(g.phase == Phase::GameOver || rolls >= 5, "the game went on ({rolls} rolls)");
        assert!(c[0].latest("game").is_some(), "the person at the table was sent the game as it changed");
    }

    #[test]
    fn a_bot_never_moves_for_a_person() {
        let (mut room, mut c) = table(&["Ada"]);
        c[0].say(&mut room, json!({ "type": "add_bot" }));
        c[0].say(&mut room, json!({ "type": "start" }));
        assert!(room.game.is_some());
        // Whoever is first, the bot can place its own settlement and road and then it has to wait for Ada.
        let mut now = Instant::now();
        for _ in 0..40 {
            now += Duration::from_secs(1);
            room.tick_at(now);
        }
        let g = room.game.as_ref().unwrap();
        assert_eq!(g.current, HOST, "it is Ada's move, and nobody makes it for her");
        assert!(g.buildings.iter().flatten().all(|b| b.owner != HOST));
        assert!(g.roads.iter().flatten().all(|&r| r != HOST));
        assert!(g.buildings.iter().flatten().count() <= 1);
    }
}
