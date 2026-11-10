mod board;
mod builtin;
mod game;
mod log_event;
mod map_def;
mod map_files;
mod map_loader;
mod maps;
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
use maps::SharedMaps;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tower_http::services::{ServeDir, ServeFile};

type Tx = UnboundedSender<String>;
type Shared = Arc<Mutex<Room>>;

/// Seats at the table (the engine's limit; a map can allow fewer, see `PlayerRange`).
const MAX_PLAYERS: usize = map_def::MAX_PLAYERS;

/// Messages for joining and managing the room (everything else is a game `Action`).
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Control {
    Join { name: String, color: Option<String>, token: Option<String> },
    Start,
    /// Host only, before the game starts: pick the turn timer mode.
    SetTimer { mode: TimerMode },
    /// Host only, before the game starts: replace the game rules (the whole `Settings` object).
    SetSettings { settings: Settings },
    /// Host only, before the game starts: pick the map, by the id the lobby lists it under.
    SetMap { map: String },
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
    connected: bool,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMsg<'a> {
    Welcome { token: &'a str },
    /// `map` is the id of the chosen map; `maps` is every map the host can choose from.
    Lobby { you: usize, players: Vec<LobbyPlayer>, timer: TimerMode, settings: Settings, map: &'a str, maps: &'a [maps::MapInfo] },
    Game { view: game::GameView<'a> },
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
}

/// The single in-memory table. Nothing here is ever written to disk.
struct Room {
    seats: Vec<Seat>,
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
}

impl Default for Room {
    fn default() -> Room {
        Room {
            seats: Vec::new(),
            game: None,
            timer: TimerMode::default(),
            settings: Settings::default(),
            map: builtin::DEFAULT_MAP.to_string(),
            maps: maps::shared_with_builtins(),
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

    fn seat_of(&self, token: &str) -> Option<usize> {
        self.seats.iter().position(|s| s.token == token)
    }

    fn broadcast(&self) {
        let connected: Vec<bool> = self.seats.iter().map(|s| s.tx.is_some()).collect();
        // Only a lobby lists the maps (a game just carries its board), so only then ask the registry.
        let maps = if self.game.is_none() { maps::infos(&self.maps) } else { Vec::new() };
        for (i, seat) in self.seats.iter().enumerate() {
            let Some(tx) = &seat.tx else { continue };
            match &self.game {
                Some(g) => send(tx, &ServerMsg::Game { view: g.view(i, &connected) }),
                None => send(
                    tx,
                    &ServerMsg::Lobby {
                        you: i,
                        timer: self.timer,
                        settings: self.settings,
                        map: &self.map,
                        maps: &maps,
                        players: self
                            .seats
                            .iter()
                            .map(|s| LobbyPlayer {
                                name: s.name.clone(),
                                color: s.color.to_string(),
                                connected: s.tx.is_some(),
                            })
                            .collect(),
                    },
                ),
            }
        }
    }

    fn handle(&mut self, tx: &Tx, token: &mut Option<String>, msg: ClientMsg) {
        match msg {
            ClientMsg::Control(Control::Join { name, color, token: old }) => {
                if token.is_some() {
                    return;
                }
                // Reconnect to an existing seat?
                if let Some(i) = old.as_deref().and_then(|t| self.seat_of(t)) {
                    self.seats[i].tx = Some(tx.clone());
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
                });
                send(tx, &ServerMsg::Welcome { token: &t });
                *token = Some(t);
                self.broadcast();
            }
            ClientMsg::Control(Control::Start) => {
                let Some(me) = token.as_deref().and_then(|t| self.seat_of(t)) else {
                    return error(tx, "Join the table first");
                };
                if me != 0 {
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
                        self.broadcast();
                    }
                    Err(e) => return error(tx, format!("Can't start on this map: {e}")),
                }
            }
            ClientMsg::Control(Control::SetTimer { mode }) => {
                let Some(me) = token.as_deref().and_then(|t| self.seat_of(t)) else {
                    return error(tx, "Join the table first");
                };
                if me != 0 {
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
                if me != 0 {
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
            ClientMsg::Control(Control::SetSettings { settings }) => {
                let Some(me) = token.as_deref().and_then(|t| self.seat_of(t)) else {
                    return error(tx, "Join the table first");
                };
                if me != 0 {
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
                self.seats.retain(|s| s.tx.is_some());
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

    /// Fire any countdown that has run out and tell everyone if the game changed.
    fn tick(&mut self) {
        let changed = match self.game.as_mut() {
            Some(g) => g.tick(Instant::now()),
            None => false,
        };
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
            Some(0) => {}
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
        let room = room.lock().unwrap();
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
    // `catan-lan validate map.json ...` checks map files and exits; it never starts the server.
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

    println!("Catan LAN server running");
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
        let (room, mut c) = table(&["Ada", "Bo"]);
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
}
