// ayynoxs map pack: a minimap with waypoints over Horizon's own minimap,
// and a world map of every chunk the player has seen, kept on disk per
// world, dimension and layer. Both share the waypoints and the language.
//
// Everything runs on the window's thread; the state lives here, never in
// `Game`'s fields.
use super::{
    Game, Screen,
    native_hooks::{NativeMinimap, NativeMod, NativePlayer},
};
use horizon_client::{
    blocks::{BlockFacts, Fluid, MAP_COLORS},
    chunk::{Chunk, MIN_Y, SECTIONS},
    extras::Corner,
    extras_hud::Corners,
    hud::{HudBatch, rgba},
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

pub(crate) const MINIMAP: &str = "ayynoxs_minimap";
pub(crate) const WORLDMAP: &str = "ayynoxs_worldmap";
pub(crate) const VERSION: &str = "0.3.0";

const KEY_WAYPOINT: &str = "waypoint";
const KEY_ZOOM_IN: &str = "zoom_in";
const KEY_ZOOM_OUT: &str = "zoom_out";
const KEY_OPEN: &str = "open";

/// Horizon's HUD boxes keep this far from the edge (`extras_hud::MARGIN`).
const MARGIN: f32 = 2.0;
/// The minimap's image side in texels (`minimap::SIZE`).
const IMAGE: f32 = 128.0;
/// CPU time the explorer may take per frame.
const SCAN_BUDGET: Duration = Duration::from_micros(1500);
/// Chunks this close to the player are rescanned after block changes.
const NEAR: i32 = 3;
const RESCAN: Duration = Duration::from_secs(2);
const SAVE_EVERY: Duration = Duration::from_secs(15);
/// A new layer must hold this long before the map follows it, so that
/// walking under an overhang doesn't flip it back and forth.
const LAYER_SETTLE: Duration = Duration::from_millis(750);
/// Cave layers are kept per band of this many blocks of head height.
const BAND: i32 = 16;
/// The world map's columns at most; zoomed out, a cell samples one block.
const MAX_COLUMNS: f32 = 320.0;

const SIZES: [f32; 4] = [64.0, 96.0, 128.0, 160.0];
const ZOOMS: [f32; 5] = [0.5, 1.0, 1.5, 2.0, 4.0];
const CORNERS: [Corner; 4] = [
    Corner::TopLeft,
    Corner::TopRight,
    Corner::BottomLeft,
    Corner::BottomRight,
];
/// Waypoint colours, ARGB.
const COLORS: [u32; 8] = [
    0xFFFF5555, 0xFF55FF55, 0xFF5599FF, 0xFFFFFF55, 0xFFFF55FF, 0xFF55FFFF, 0xFFFFAA00, 0xFFFFFFFF,
];

// --- Languages ---------------------------------------------------------

/// Each language's own name, in the order of every row of `tr`.
const LANGS: [&str; 5] = ["English", "Deutsch", "Français", "Italiano", "Русский"];

#[derive(Clone, Copy)]
enum T {
    WorldTitle,
    MiniDesc,
    WorldDesc,
    KeyWaypoint,
    KeyZoomIn,
    KeyZoomOut,
    KeyOpen,
    Language,
    Corner,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Size,
    Zoom,
    BlocksPerPixel,
    Shape,
    Round,
    Square,
    Rotate,
    Coordinates,
    Waypoints,
    On,
    Off,
    Explore,
    Grid,
    MapWaypoints,
    WaypointN,
    DeathPoint,
    Prefix,
    MsgSet,
    MsgDeleted,
    MsgCleared,
    MsgNone,
    OtherDim,
    South,
    SouthWest,
    West,
    NorthWest,
    North,
    NorthEast,
    East,
    SouthEast,
    Overworld,
    Nether,
    End,
    Surface,
    Cave,
    Hint,
    PxPerBlock,
    BlocksPerPx,
    InWorld,
    MsgRemoved,
    MsgRenamed,
    MsgExists,
    MsgNotFound,
    MsgHelp,
}

/// `t` in language `lang` (an index of `LANGS`).
fn tr(lang: usize, t: T) -> &'static str {
    let row: [&str; 5] = match t {
        T::WorldTitle => ["ayynoxs world map", "ayynoxs weltkarte", "ayynoxs carte du monde", "ayynoxs mappa del mondo", "ayynoxs карта мира"],
        T::MiniDesc => [
            "Minimap with coordinates, direction, waypoints and a death point. /wp add|del|list|clear",
            "Minimap mit Koordinaten, Himmelsrichtung, Wegpunkten und Todespunkt. /wp add|del|list|clear",
            "Minicarte avec coordonnées, direction, points de passage et point de mort. /wp add|del|list|clear",
            "Minimappa con coordinate, direzione, waypoint e punto di morte. /wp add|del|list|clear",
            "Миникарта с координатами, направлением, метками и точкой смерти. /wp add|del|list|clear",
        ],
        T::WorldDesc => [
            "Full-screen map of every chunk you explored, saved per world and layer.",
            "Vollbild-Karte aller erkundeten Chunks, pro Welt und Ebene gespeichert.",
            "Carte plein écran de tous les chunks explorés, enregistrée par monde et par couche.",
            "Mappa a schermo intero di tutti i chunk esplorati, salvata per mondo e livello.",
            "Полноэкранная карта всех исследованных чанков, по мирам и слоям.",
        ],
        T::KeyWaypoint => ["Set waypoint", "Wegpunkt setzen", "Placer un point", "Imposta waypoint", "Поставить метку"],
        T::KeyZoomIn => ["Zoom in", "Hineinzoomen", "Zoom avant", "Ingrandisci", "Приблизить"],
        T::KeyZoomOut => ["Zoom out", "Herauszoomen", "Zoom arrière", "Riduci", "Отдалить"],
        T::KeyOpen => ["Open world map", "Weltkarte öffnen", "Ouvrir la carte", "Apri la mappa", "Открыть карту мира"],
        T::Language => ["Language", "Sprache", "Langue", "Lingua", "Язык"],
        T::Corner => ["Corner", "Ecke", "Coin", "Angolo", "Угол"],
        T::TopLeft => ["Top left", "Oben links", "En haut à gauche", "In alto a sinistra", "Сверху слева"],
        T::TopRight => ["Top right", "Oben rechts", "En haut à droite", "In alto a destra", "Сверху справа"],
        T::BottomLeft => ["Bottom left", "Unten links", "En bas à gauche", "In basso a sinistra", "Снизу слева"],
        T::BottomRight => ["Bottom right", "Unten rechts", "En bas à droite", "In basso a destra", "Снизу справа"],
        T::Size => ["Size", "Größe", "Taille", "Dimensione", "Размер"],
        T::Zoom => ["Zoom", "Zoom", "Zoom", "Zoom", "Масштаб"],
        T::BlocksPerPixel => ["blocks/pixel", "Blöcke/Pixel", "blocs/pixel", "blocchi/pixel", "блоков/пиксель"],
        T::Shape => ["Shape", "Form", "Forme", "Forma", "Форма"],
        T::Round => ["Round", "Rund", "Ronde", "Rotonda", "Круглая"],
        T::Square => ["Square", "Eckig", "Carrée", "Quadrata", "Квадратная"],
        T::Rotate => ["Rotate with player", "Mitdrehen", "Tourner avec le joueur", "Ruota con il giocatore", "Вращать с игроком"],
        T::Coordinates => ["Coordinates", "Koordinaten", "Coordonnées", "Coordinate", "Координаты"],
        T::Waypoints => ["Waypoints", "Wegpunkte", "Points de passage", "Waypoint", "Метки"],
        T::On => ["On", "An", "Activé", "Attivo", "Вкл"],
        T::Off => ["Off", "Aus", "Désactivé", "Disattivo", "Выкл"],
        T::Explore => ["Explore & save", "Erkunden & speichern", "Explorer et enregistrer", "Esplora e salva", "Исследовать и сохранять"],
        T::Grid => ["Chunk grid", "Chunk-Raster", "Grille des chunks", "Griglia dei chunk", "Сетка чанков"],
        T::MapWaypoints => ["Waypoints on the map", "Wegpunkte auf der Karte", "Points sur la carte", "Waypoint sulla mappa", "Метки на карте"],
        T::WaypointN => ["Waypoint {0}", "Wegpunkt {0}", "Point {0}", "Waypoint {0}", "Метка {0}"],
        T::DeathPoint => ["Death point", "Todespunkt", "Point de mort", "Punto di morte", "Точка смерти"],
        T::Prefix => ["[Map]", "[Karte]", "[Carte]", "[Mappa]", "[Карта]"],
        T::MsgSet => [
            "Waypoint §e{0}§r set at {1}",
            "Wegpunkt §e{0}§r gesetzt bei {1}",
            "Point §e{0}§r placé en {1}",
            "Waypoint §e{0}§r impostato a {1}",
            "Метка §e{0}§r поставлена: {1}",
        ],
        T::MsgDeleted => ["{0} waypoint(s) deleted", "{0} Wegpunkt(e) gelöscht", "{0} point(s) supprimé(s)", "{0} waypoint eliminati", "Удалено меток: {0}"],
        T::MsgCleared => [
            "All waypoints of this world deleted",
            "Alle Wegpunkte dieser Welt gelöscht",
            "Tous les points de ce monde ont été supprimés",
            "Tutti i waypoint di questo mondo eliminati",
            "Все метки этого мира удалены",
        ],
        T::MsgNone => [
            "No waypoints yet. /wp add <name> sets one.",
            "Keine Wegpunkte. /wp add <Name> setzt einen.",
            "Aucun point. /wp add <nom> en place un.",
            "Nessun waypoint. /wp add <nome> ne imposta uno.",
            "Меток нет. /wp add <имя> ставит метку.",
        ],
        T::OtherDim => ["other dimension", "andere Dimension", "autre dimension", "altra dimensione", "другое измерение"],
        T::South => ["South", "Süden", "Sud", "Sud", "Юг"],
        T::SouthWest => ["Southwest", "Südwesten", "Sud-ouest", "Sud-ovest", "Юго-запад"],
        T::West => ["West", "Westen", "Ouest", "Ovest", "Запад"],
        T::NorthWest => ["Northwest", "Nordwesten", "Nord-ouest", "Nord-ovest", "Северо-запад"],
        T::North => ["North", "Norden", "Nord", "Nord", "Север"],
        T::NorthEast => ["Northeast", "Nordosten", "Nord-est", "Nord-est", "Северо-восток"],
        T::East => ["East", "Osten", "Est", "Est", "Восток"],
        T::SouthEast => ["Southeast", "Südosten", "Sud-est", "Sud-est", "Юго-восток"],
        T::Overworld => ["Overworld", "Oberwelt", "Surface", "Overworld", "Верхний мир"],
        T::Nether => ["Nether", "Nether", "Nether", "Nether", "Незер"],
        T::End => ["The End", "Ende", "L'End", "L'End", "Энд"],
        T::Surface => ["Surface", "Oberfläche", "Surface", "Superficie", "Поверхность"],
        T::Cave => ["Cave, Y {0}", "Höhle, Y {0}", "Grotte, Y {0}", "Grotta, Y {0}", "Пещера, Y {0}"],
        T::Hint => [
            "Drag/WASD: move · Wheel: zoom · Space: to you · B: waypoint · Del: remove · M/Esc: close",
            "Ziehen/WASD: bewegen · Mausrad: Zoom · Leertaste: zu dir · B: Wegpunkt · Entf: löschen · M/Esc: schließen",
            "Glisser/ZQSD : déplacer · Molette : zoom · Espace : vers toi · B : point · Suppr : effacer · M/Échap : fermer",
            "Trascina/WASD: muovi · Rotella: zoom · Spazio: da te · B: waypoint · Canc: elimina · M/Esc: chiudi",
            "Тащить/WASD: двигать · Колесо: масштаб · Пробел: к себе · B: метка · Del: удалить · M/Esc: закрыть",
        ],
        T::PxPerBlock => ["{0} px/block", "{0} px/Block", "{0} px/bloc", "{0} px/blocco", "{0} px/блок"],
        T::BlocksPerPx => ["1 px = {0} blocks", "1 px = {0} Blöcke", "1 px = {0} blocs", "1 px = {0} blocchi", "1 px = {0} блоков"],
        T::InWorld => ["Waypoints in the world", "Wegpunkte in der Welt", "Points dans le monde", "Waypoint nel mondo", "Метки в мире"],
        T::MsgRemoved => [
            "Waypoint §e{0}§r removed",
            "Wegpunkt §e{0}§r gelöscht",
            "Point §e{0}§r supprimé",
            "Waypoint §e{0}§r eliminato",
            "Метка §e{0}§r удалена",
        ],
        T::MsgRenamed => [
            "Waypoint renamed to §e{0}§r",
            "Wegpunkt umbenannt in §e{0}§r",
            "Point renommé en §e{0}§r",
            "Waypoint rinominato in §e{0}§r",
            "Метка переименована в §e{0}§r",
        ],
        T::MsgExists => [
            "There is already a waypoint here: §e{0}§r. Press B on it again to remove it.",
            "Hier ist schon ein Wegpunkt: §e{0}§r. Drück dort nochmal B, um ihn zu löschen.",
            "Il y a déjà un point ici : §e{0}§r. Appuie encore sur B pour le supprimer.",
            "Qui c'è già un waypoint: §e{0}§r. Premi di nuovo B per eliminarlo.",
            "Здесь уже есть метка: §e{0}§r. Нажми B ещё раз, чтобы удалить её.",
        ],
        T::MsgNotFound => [
            "No waypoint found. /wp list shows them all.",
            "Kein Wegpunkt gefunden. /wp list zeigt alle.",
            "Aucun point trouvé. /wp list les affiche tous.",
            "Nessun waypoint trovato. /wp list li mostra tutti.",
            "Метка не найдена. /wp list покажет все.",
        ],
        T::MsgHelp => [
            "B: set a waypoint (type its name, Enter) or remove the one you stand on · /wp add <name> · /wp del [name|nr] (none: the nearest) · /wp rename <nr> <name> · /wp list · /wp clear",
            "B: Wegpunkt setzen (Namen tippen, Enter) oder den, auf dem du stehst, löschen · /wp add <Name> · /wp del [Name|Nr] (ohne: den nächsten) · /wp rename <Nr> <Name> · /wp list · /wp clear",
            "B : placer un point (tape son nom, Entrée) ou supprimer celui où tu es · /wp add <nom> · /wp del [nom|n°] (sans : le plus proche) · /wp rename <n°> <nom> · /wp list · /wp clear",
            "B: imposta un waypoint (scrivi il nome, Invio) o elimina quello su cui sei · /wp add <nome> · /wp del [nome|nr] (senza: il più vicino) · /wp rename <nr> <nome> · /wp list · /wp clear",
            "B: поставить метку (введи имя, Enter) или удалить ту, на которой стоишь · /wp add <имя> · /wp del [имя|№] (без: ближайшую) · /wp rename <№> <имя> · /wp list · /wp clear",
        ],
    };
    row[lang.min(LANGS.len() - 1)]
}
/// `template` with `{0}`, `{1}`… replaced by `args`.
fn fill(template: &str, args: &[&str]) -> String {
    let mut out = template.to_owned();
    for (i, a) in args.iter().enumerate() {
        out = out.replace(&format!("{{{i}}}"), a);
    }
    out
}

// --- State -------------------------------------------------------------

#[derive(Clone, Copy)]
struct MiniSettings {
    corner: usize,
    size: usize,
    zoom: usize,
    round: bool,
    rotate: bool,
    coords: bool,
    waypoints: bool,
    /// Waypoints shown in the world with their distance.
    world: bool,
}
impl Default for MiniSettings {
    fn default() -> Self {
        Self {
            corner: 1,
            size: 1,
            zoom: 1,
            round: false,
            rotate: false,
            coords: true,
            waypoints: true,
            world: true,
        }
    }
}

#[derive(Clone, Copy)]
struct MapSettings {
    explore: bool,
    grid: bool,
    waypoints: bool,
}
impl Default for MapSettings {
    fn default() -> Self {
        Self {
            explore: true,
            grid: false,
            waypoints: true,
        }
    }
}

#[derive(Clone, Debug)]
struct Waypoint {
    name: String,
    x: i32,
    y: i32,
    z: i32,
    dim: String,
    color: usize,
    death: bool,
}

/// One chunk of the map: per column `id << 2 | brightness` (0 unknown),
/// and its height.
struct Tile {
    color: [u8; 256],
    height: [i16; 256],
}

/// 32 x 32 chunks, as region files are.
struct Region {
    tiles: Vec<Option<Box<Tile>>>,
    dirty: bool,
}
impl Region {
    fn new() -> Self {
        Self {
            tiles: (0..1024).map(|_| None).collect(),
            dirty: false,
        }
    }
}

/// What a scan needs of each block state: map colour and flags.
struct Palette {
    color: Vec<u8>,
    flags: Vec<u8>,
}
const WATER_FLAG: u8 = 1;
const LAVA_FLAG: u8 = 2;
const FLUID: u8 = 3;
const AIR: u8 = 4;
const STURDY_TOP: u8 = 8;
/// `MapColor.WATER`, `FIRE`.
const WATER: u8 = 12;
const LAVA: u8 = 4;
impl Palette {
    fn new(facts: &BlockFacts) -> Self {
        let mut color = Vec::with_capacity(facts.states.len());
        let mut flags = Vec::with_capacity(facts.states.len());
        for f in &facts.states {
            color.push(f.map_color);
            let mut b = match f.fluid {
                Fluid::None => 0,
                Fluid::Water => WATER_FLAG,
                Fluid::Lava => LAVA_FLAG,
            };
            if matches!(
                &*f.name,
                "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
            ) {
                b |= AIR;
            }
            if f.sturdy_full >> 1 & 1 != 0 {
                b |= STURDY_TOP;
            }
            flags.push(b);
        }
        Self { color, flags }
    }
    #[inline]
    fn get(&self, id: u32) -> (u8, u8) {
        let i = id as usize;
        (
            self.color.get(i).copied().unwrap_or(0),
            self.flags.get(i).copied().unwrap_or(AIR),
        )
    }
}

/// Which layer of a dimension the map shows, as Horizon's minimap picks
/// it: the surface under the open sky, else (underground, under a roof, in
/// the Nether) the floor below the player's head, walls cut at it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Layer {
    Surface,
    /// The band `head / BAND` of head heights.
    Cave(i32),
}
impl Layer {
    fn of(cut: Option<i32>) -> Self {
        cut.map_or(Layer::Surface, |head| Layer::Cave(head.div_euclid(BAND)))
    }
    /// Its folder under the dimension's: the surface keeps 0.1.0's.
    fn folder(&self) -> Option<String> {
        match self {
            Layer::Surface => None,
            Layer::Cave(band) => Some(format!("cave_{band}")),
        }
    }
}

/// The world's key (`sp_<folder>`, `mp_<address>`), dimension and layer.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Place {
    world: String,
    dim: String,
    layer: Layer,
}

#[derive(Default)]
struct Explorer {
    /// Chunks scanned: the chunk's allocation (a new one is a new chunk),
    /// the world's revision then and when.
    scanned: HashMap<(i32, i32), (usize, u64, Instant)>,
    regions: HashMap<(i32, i32), Region>,
    /// Regions with no file yet.
    missing: HashSet<(i32, i32)>,
    dir: Option<PathBuf>,
    frames: u32,
}

struct MapScreen {
    cx: f64,
    cz: f64,
    /// GUI pixels per block.
    zoom: f32,
    drag: Option<(f32, f32)>,
    /// Up, down, left, right held.
    held: [bool; 4],
    last: Instant,
}

struct State {
    loaded: bool,
    lang: usize,
    mini: MiniSettings,
    map: MapSettings,
    place: Option<Place>,
    /// The head height the current cave layer is cut at; none on the surface.
    cut: Option<i32>,
    /// A layer the player moved into, and since when.
    pending: Option<(Layer, Instant)>,
    server: String,
    waypoints: Vec<Waypoint>,
    dead: bool,
    /// The waypoint key asked for a name: the chat opens on the next frame,
    /// so the key's own character doesn't land in it.
    name_prompt: bool,
    /// The scan's palette, for the world's block states.
    palette: Option<Palette>,
    explorer: Explorer,
    screen: Option<MapScreen>,
    last_save: Instant,
}
impl Default for State {
    fn default() -> Self {
        Self {
            loaded: false,
            lang: 0,
            mini: MiniSettings::default(),
            map: MapSettings::default(),
            place: None,
            cut: None,
            pending: None,
            server: String::new(),
            waypoints: Vec::new(),
            dead: false,
            name_prompt: false,
            palette: None,
            explorer: Explorer::default(),
            screen: None,
            last_save: Instant::now(),
        }
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}
fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

// --- Settings ----------------------------------------------------------

fn flag(game: &Game<'_>, id: &str, key: &str, default: bool) -> bool {
    game.native_setting(id, key)
        .map_or(default, |v| v == "true")
}
fn index(game: &Game<'_>, id: &str, key: &str, default: usize, len: usize) -> usize {
    game.native_setting(id, key)
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&i| i < len)
        .unwrap_or(default)
}
fn load_settings(s: &mut State, game: &Game<'_>) {
    s.lang = index(game, MINIMAP, "lang", 0, LANGS.len());
    let d = MiniSettings::default();
    s.mini = MiniSettings {
        corner: index(game, MINIMAP, "corner", d.corner, CORNERS.len()),
        size: index(game, MINIMAP, "size", d.size, SIZES.len()),
        zoom: index(game, MINIMAP, "zoom", d.zoom, ZOOMS.len()),
        round: flag(game, MINIMAP, "round", d.round),
        rotate: flag(game, MINIMAP, "rotate", d.rotate),
        coords: flag(game, MINIMAP, "coords", d.coords),
        waypoints: flag(game, MINIMAP, "waypoints", d.waypoints),
        world: flag(game, MINIMAP, "world", d.world),
    };
    let m = MapSettings::default();
    s.map = MapSettings {
        explore: flag(game, WORLDMAP, "explore", m.explore),
        grid: flag(game, WORLDMAP, "grid", m.grid),
        waypoints: flag(game, WORLDMAP, "waypoints", m.waypoints),
    };
    s.loaded = true;
}
fn ensure_settings(s: &mut State, game: &Game<'_>) {
    if !s.loaded {
        load_settings(s, game);
    }
}
fn store_settings(s: &State, game: &Game<'_>) {
    let m = s.mini;
    let pairs = [
        ("lang", s.lang.to_string()),
        ("corner", m.corner.to_string()),
        ("size", m.size.to_string()),
        ("zoom", m.zoom.to_string()),
        ("round", m.round.to_string()),
        ("rotate", m.rotate.to_string()),
        ("coords", m.coords.to_string()),
        ("waypoints", m.waypoints.to_string()),
        ("world", m.world.to_string()),
    ];
    for (k, v) in pairs {
        game.native_set_setting(MINIMAP, k, &v);
    }
    let w = s.map;
    for (k, v) in [
        ("explore", w.explore),
        ("grid", w.grid),
        ("waypoints", w.waypoints),
    ] {
        game.native_set_setting(WORLDMAP, k, &v.to_string());
    }
}
fn on_off(lang: usize, b: bool) -> &'static str {
    tr(lang, if b { T::On } else { T::Off })
}

/// Options > Mods: both halves of the pack, in the chosen language (the
/// list is made when the game starts).
pub(super) fn listed(game: &Game<'_>, mods: &mut Vec<NativeMod>) {
    let lang = with(|s| {
        ensure_settings(s, game);
        s.lang
    });
    mods.push(NativeMod {
        id: MINIMAP.into(),
        title: format!("ayynoxs minimap {VERSION}"),
        description: tr(lang, T::MiniDesc).into(),
        keys: vec![
            [KEY_WAYPOINT.into(), tr(lang, T::KeyWaypoint).into(), "key.keyboard.b".into()],
            [KEY_ZOOM_IN.into(), tr(lang, T::KeyZoomIn).into(), "key.keyboard.i".into()],
            [KEY_ZOOM_OUT.into(), tr(lang, T::KeyZoomOut).into(), "key.keyboard.o".into()],
        ],
    });
    mods.push(NativeMod {
        id: WORLDMAP.into(),
        title: format!("{} {VERSION}", tr(lang, T::WorldTitle)),
        description: tr(lang, T::WorldDesc).into(),
        keys: vec![[KEY_OPEN.into(), tr(lang, T::KeyOpen).into(), "key.keyboard.m".into()]],
    });
}

/// A settings page's buttons; the language is the last of both pages.
pub(super) fn settings_page(game: &Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    with(|s| {
        ensure_settings(s, game);
        let l = s.lang;
        let language = format!("{}: {}", tr(l, T::Language), LANGS[l]);
        if mod_id == MINIMAP {
            let m = s.mini;
            let corner = [T::TopLeft, T::TopRight, T::BottomLeft, T::BottomRight][m.corner];
            let shape = if m.round { T::Round } else { T::Square };
            buttons.push(format!("{}: {}", tr(l, T::Corner), tr(l, corner)));
            buttons.push(format!("{}: {}", tr(l, T::Size), SIZES[m.size] as u32));
            buttons.push(format!("{}: {} {}", tr(l, T::Zoom), ZOOMS[m.zoom], tr(l, T::BlocksPerPixel)));
            buttons.push(format!("{}: {}", tr(l, T::Shape), tr(l, shape)));
            buttons.push(format!("{}: {}", tr(l, T::Rotate), on_off(l, m.rotate)));
            buttons.push(format!("{}: {}", tr(l, T::Coordinates), on_off(l, m.coords)));
            buttons.push(format!("{}: {}", tr(l, T::Waypoints), on_off(l, m.waypoints)));
            buttons.push(format!("{}: {}", tr(l, T::InWorld), on_off(l, m.world)));
            buttons.push(language);
        } else if mod_id == WORLDMAP {
            let m = s.map;
            buttons.push(format!("{}: {}", tr(l, T::Explore), on_off(l, m.explore)));
            buttons.push(format!("{}: {}", tr(l, T::Grid), on_off(l, m.grid)));
            buttons.push(format!("{}: {}", tr(l, T::MapWaypoints), on_off(l, m.waypoints)));
            buttons.push(language);
        }
    });
}

pub(super) fn settings_click(game: &Game<'_>, mod_id: &str, button: usize) {
    with(|s| {
        ensure_settings(s, game);
        let next_lang = (s.lang + 1) % LANGS.len();
        if mod_id == MINIMAP {
            let m = &mut s.mini;
            match button {
                0 => m.corner = (m.corner + 1) % CORNERS.len(),
                1 => m.size = (m.size + 1) % SIZES.len(),
                2 => m.zoom = (m.zoom + 1) % ZOOMS.len(),
                3 => m.round = !m.round,
                4 => m.rotate = !m.rotate,
                5 => m.coords = !m.coords,
                6 => m.waypoints = !m.waypoints,
                7 => m.world = !m.world,
                8 => s.lang = next_lang,
                _ => return,
            }
        } else if mod_id == WORLDMAP {
            let m = &mut s.map;
            match button {
                0 => m.explore = !m.explore,
                1 => m.grid = !m.grid,
                2 => m.waypoints = !m.waypoints,
                3 => s.lang = next_lang,
                _ => return,
            }
        } else {
            return;
        }
        store_settings(s, game);
    });
}

// --- World, waypoints, saving -----------------------------------------

fn sanitize(text: &str) -> String {
    let s: String = text
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' })
        .take(80)
        .collect();
    if s.is_empty() { "unnamed".into() } else { s }
}
fn world_key(s: &State, game: &Game<'_>) -> String {
    if game.remote.is_some() {
        format!("mp_{}", sanitize(&s.server))
    } else {
        let name = game
            .world_root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        format!("sp_{}", sanitize(&name))
    }
}
fn waypoint_file(game: &Game<'_>, world: &str) -> PathBuf {
    game.native_data_dir(MINIMAP)
        .join("waypoints")
        .join(format!("{world}.txt"))
}
fn load_waypoints(game: &Game<'_>, world: &str) -> Vec<Waypoint> {
    let Ok(text) = fs::read_to_string(waypoint_file(game, world)) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 7 {
                return None;
            }
            Some(Waypoint {
                name: f[0].to_owned(),
                x: f[1].parse().ok()?,
                y: f[2].parse().ok()?,
                z: f[3].parse().ok()?,
                dim: f[4].to_owned(),
                color: f[5].parse::<usize>().ok()? % COLORS.len(),
                death: f[6] == "1",
            })
        })
        .collect()
}
fn save_waypoints(s: &State, game: &Game<'_>) {
    let Some(place) = &s.place else {
        return;
    };
    let path = waypoint_file(game, &place.world);
    let mut out = String::new();
    for w in &s.waypoints {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            w.name,
            w.x,
            w.y,
            w.z,
            w.dim,
            w.color,
            u8::from(w.death)
        ));
    }
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(path, out);
}
fn clean_name(name: &str) -> String {
    let n: String = name
        .chars()
        .filter(|c| !c.is_control())
        .take(32)
        .collect();
    n.trim().to_owned()
}
fn chat(s: &State, game: &mut Game<'_>, text: &str) {
    game.native_chat(&format!("§a{}§r {text}", tr(s.lang, T::Prefix)));
}
/// The waypoint of `dim` nearest to `(x, y, z)` within `reach` blocks
/// (horizontally; 4 blocks up or down), by index.
fn nearest(s: &State, dim: &str, (x, y, z): (f64, f64, f64), reach: f64) -> Option<usize> {
    s.waypoints
        .iter()
        .enumerate()
        .filter(|(_, w)| w.dim == dim && (f64::from(w.y) - y).abs() <= 4.0)
        .map(|(i, w)| (i, (f64::from(w.x) + 0.5 - x).hypot(f64::from(w.z) + 0.5 - z)))
        .filter(|&(_, d)| d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}
fn remove_waypoint(s: &mut State, game: &mut Game<'_>, i: usize) {
    let w = s.waypoints.remove(i);
    save_waypoints(s, game);
    chat(s, game, &fill(tr(s.lang, T::MsgRemoved), &[&w.name]));
}
/// A new waypoint at `at`; on an existing one (not a death point) a name
/// renames it instead of stacking a second one there.
fn add_waypoint(s: &mut State, game: &mut Game<'_>, name: &str, at: (i32, i32, i32), dim: &str) {
    let spot = (f64::from(at.0) + 0.5, f64::from(at.1), f64::from(at.2) + 0.5);
    if let Some(i) = nearest(s, dim, spot, 1.5).filter(|&i| !s.waypoints[i].death) {
        let name = clean_name(name);
        if name.is_empty() {
            let existing = s.waypoints[i].name.clone();
            chat(s, game, &fill(tr(s.lang, T::MsgExists), &[&existing]));
        } else {
            s.waypoints[i].name = name.clone();
            save_waypoints(s, game);
            chat(s, game, &fill(tr(s.lang, T::MsgRenamed), &[&name]));
        }
        return;
    }
    let name = if clean_name(name).is_empty() {
        let n = (s.waypoints.iter().filter(|w| !w.death).count() + 1).to_string();
        fill(tr(s.lang, T::WaypointN), &[&n])
    } else {
        clean_name(name)
    };
    let color = s.waypoints.len() % COLORS.len();
    let pos = format!("{} {} {}", at.0, at.1, at.2);
    chat(s, game, &fill(tr(s.lang, T::MsgSet), &[&name, &pos]));
    s.waypoints.push(Waypoint {
        name,
        x: at.0,
        y: at.1,
        z: at.2,
        dim: dim.to_owned(),
        color,
        death: false,
    });
    save_waypoints(s, game);
}

fn region_path(dir: &PathBuf, (rx, rz): (i32, i32)) -> PathBuf {
    dir.join(format!("r.{rx}.{rz}.amr"))
}
/// `AMR1`, a count, then per chunk its index, 256 colours and 256 heights.
fn read_region(path: &PathBuf) -> Option<Region> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() < 6 || &bytes[..4] != b"AMR1" {
        return None;
    }
    let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
    let mut region = Region::new();
    let mut at = 6;
    const ENTRY: usize = 2 + 256 + 512;
    for _ in 0..count {
        let e = bytes.get(at..at + ENTRY)?;
        let idx = u16::from_le_bytes([e[0], e[1]]) as usize;
        if idx >= 1024 {
            return None;
        }
        let mut tile = Tile {
            color: [0; 256],
            height: [0; 256],
        };
        tile.color.copy_from_slice(&e[2..258]);
        for i in 0..256 {
            tile.height[i] = i16::from_le_bytes([e[258 + i * 2], e[259 + i * 2]]);
        }
        region.tiles[idx] = Some(Box::new(tile));
        at += ENTRY;
    }
    Some(region)
}
fn write_region(path: &PathBuf, region: &Region) {
    let mut out = Vec::with_capacity(6 + 1024 * 770);
    out.extend_from_slice(b"AMR1");
    let count = region.tiles.iter().filter(|t| t.is_some()).count() as u16;
    out.extend_from_slice(&count.to_le_bytes());
    for (i, t) in region.tiles.iter().enumerate() {
        let Some(t) = t else { continue };
        out.extend_from_slice(&(i as u16).to_le_bytes());
        out.extend_from_slice(&t.color);
        for h in t.height {
            out.extend_from_slice(&h.to_le_bytes());
        }
    }
    let tmp = path.with_extension("tmp");
    if fs::write(&tmp, &out).is_ok() {
        let _ = fs::rename(&tmp, path);
    }
}
fn flush(e: &mut Explorer) {
    let Some(dir) = e.dir.clone() else {
        return;
    };
    if e.regions.values().any(|r| r.dirty) {
        let _ = fs::create_dir_all(&dir);
    }
    for (key, region) in e.regions.iter_mut() {
        if region.dirty {
            write_region(&region_path(&dir, *key), region);
            region.dirty = false;
        }
    }
}
impl Explorer {
    /// The region at `key`, read from disk the first time.
    fn region(&mut self, key: (i32, i32)) -> Option<&Region> {
        self.load(key);
        self.regions.get(&key)
    }
    fn load(&mut self, key: (i32, i32)) {
        if self.regions.contains_key(&key) || self.missing.contains(&key) {
            return;
        }
        let Some(dir) = &self.dir else {
            return;
        };
        match read_region(&region_path(dir, key)) {
            Some(r) => {
                self.regions.insert(key, r);
            }
            None => {
                self.missing.insert(key);
            }
        }
    }
    fn region_mut(&mut self, key: (i32, i32)) -> &mut Region {
        self.load(key);
        self.missing.remove(&key);
        self.regions.entry(key).or_insert_with(Region::new)
    }
    fn tile(&self, cx: i32, cz: i32) -> Option<&Tile> {
        let r = self.regions.get(&(cx >> 5, cz >> 5))?;
        r.tiles[((cz & 31) * 32 + (cx & 31)) as usize].as_deref()
    }
    /// A block column: (colour byte, height); 0 is unknown.
    fn column(&self, x: i32, z: i32) -> (u8, i16) {
        match self.tile(x >> 4, z >> 4) {
            Some(t) => {
                let i = ((z & 15) * 16 + (x & 15)) as usize;
                (t.color[i], t.height[i])
            }
            None => (0, 0),
        }
    }
}

/// A new world, dimension or layer, or none: save the old, start the new.
fn switch_place(s: &mut State, game: &Game<'_>, now: Option<Place>) {
    if s.place == now {
        return;
    }
    flush(&mut s.explorer);
    s.explorer = Explorer::default();
    let same_world = matches!((&s.place, &now), (Some(a), Some(b)) if a.world == b.world);
    if !same_world {
        s.palette = None;
    }
    if let Some(place) = &now {
        let mut dir = game
            .native_data_dir(WORLDMAP)
            .join("worlds")
            .join(&place.world)
            .join(sanitize(&place.dim));
        if let Some(sub) = place.layer.folder() {
            dir = dir.join(sub);
        }
        s.explorer.dir = Some(dir);
        if !same_world {
            s.waypoints = load_waypoints(game, &place.world);
        }
    } else {
        s.waypoints.clear();
        s.screen = None;
        s.pending = None;
    }
    s.place = now;
}

/// The cave cut the player is at (`game/extras.rs`, `update_minimap`): the
/// head's height when it has no sky light or in the Nether, else none.
fn cut_now(game: &Game<'_>, x: f64, y: f64, z: f64, nether: bool) -> Option<i32> {
    let eye = game
        .player
        .as_ref()
        .map_or(1.62, |m| f64::from(m.dimensions.eye_height));
    let head = (y + eye).floor() as i32;
    let covered = game.world.light(x.floor() as i32, head, z.floor() as i32) >> 4 == 0;
    (nether || covered).then_some(head)
}

// --- Exploring ---------------------------------------------------------

fn top_of(chunk: &Chunk, air: u32) -> i32 {
    (0..SECTIONS)
        .rev()
        .find(|&s| chunk.sections.get(s).is_some_and(|sec| !sec.is_uniform(air)))
        .map_or(MIN_Y - 1, |s| MIN_Y + s as i32 * 16 + 15)
}
/// One column as `MapItem.update` sees it: the first block with a map
/// colour going down, a fluid's colour unless the block is sturdy on top,
/// and the fluid's depth. With a cave `cut`, a solid block at that height
/// is a wall (true) and the floor below it is scanned, as Horizon's minimap
/// does (`minimap::scan_column`).
fn scan_column(chunk: &Chunk, p: &Palette, x: i32, z: i32, top: i32, cut: Option<i32>) -> (u8, i16, u8, bool) {
    let mut y = top;
    if let Some(head) = cut {
        let (color, flags) = p.get(chunk.get(x, head, z));
        if flags & AIR == 0 && color != 0 && flags & FLUID == 0 {
            return (color, head as i16, 0, true);
        }
        y = top.min(head);
    }
    while y > MIN_Y {
        let (mut color, flags) = p.get(chunk.get(x, y, z));
        if color == 0 {
            y -= 1;
            continue;
        }
        let mut depth = 0u8;
        let fluid = flags & FLUID;
        if fluid != 0 {
            let mut below = y - 1;
            while below > MIN_Y && p.get(chunk.get(x, below, z)).1 & FLUID != 0 {
                depth = depth.saturating_add(1);
                below -= 1;
            }
            depth = depth.saturating_add(1);
            if flags & STURDY_TOP == 0 {
                color = if fluid == LAVA_FLAG { LAVA } else { WATER };
            }
        }
        return (color, y as i16, depth, false);
    }
    (0, MIN_Y as i16, 0, false)
}
/// The map item's shading: lighter facing a lower north neighbour, darker
/// facing a higher one, dithered; water by depth; walls darkest.
fn brightness(color: u8, height: i16, north: i16, depth: u8, parity: i32, wall: bool) -> u8 {
    if wall {
        return 3;
    }
    let parity = f64::from(parity & 1);
    if color == WATER {
        let diff = f64::from(depth) * 0.1 + parity * 0.2;
        return if diff < 0.5 { 2 } else if diff > 0.9 { 0 } else { 1 };
    }
    let diff = f64::from(height - north) * 4.0 / 5.0 + (parity - 0.5) * 0.4;
    if diff > 0.6 {
        2
    } else if diff < -0.6 {
        0
    } else {
        1
    }
}
fn scan_tile(chunk: &Chunk, p: &Palette, air: u32, cut: Option<i32>, north: Option<&Tile>) -> Option<Tile> {
    let top = top_of(chunk, air);
    if top < MIN_Y {
        return None;
    }
    let mut ids = [0u8; 256];
    let mut depth = [0u8; 256];
    let mut walls = [false; 256];
    let mut tile = Tile {
        color: [0; 256],
        height: [MIN_Y as i16; 256],
    };
    let mut any = false;
    for z in 0..16 {
        for x in 0..16 {
            let (c, h, d, wall) = scan_column(chunk, p, x, z, top, cut);
            let i = (z * 16 + x) as usize;
            ids[i] = c;
            tile.height[i] = h;
            depth[i] = d;
            walls[i] = wall;
            any |= c != 0;
        }
    }
    if !any {
        return None;
    }
    for z in 0..16i32 {
        for x in 0..16i32 {
            let i = (z * 16 + x) as usize;
            let id = ids[i];
            if id == 0 || usize::from(id) >= MAP_COLORS.len() {
                continue;
            }
            let h = tile.height[i];
            let n = if z > 0 {
                tile.height[i - 16]
            } else {
                north.map_or(h, |t| t.height[(15 * 16 + x) as usize])
            };
            let parity = chunk.x * 16 + x + chunk.z * 16 + z;
            tile.color[i] = id << 2 | brightness(id, h, n, depth[i], parity, walls[i]);
        }
    }
    Some(tile)
}
fn explore(s: &mut State, game: &Game<'_>, px: f64, pz: f64) {
    let cut = s.cut;
    let e = &mut s.explorer;
    if e.dir.is_none() {
        return;
    }
    let palette = s.palette.get_or_insert_with(|| Palette::new(game.res.facts()));
    let world = &game.world;
    let air = world.air.0;
    let (ccx, ccz) = ((px.floor() as i32) >> 4, (pz.floor() as i32) >> 4);
    e.frames = e.frames.wrapping_add(1);
    if e.frames % 120 == 0 {
        e.scanned.retain(|k, _| world.chunks.contains_key(k));
    }
    let radius = game.settings.render_distance.max(2) + 1;
    let started = Instant::now();
    for ring in 0..=radius {
        for dz in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs() != ring && dz.abs() != ring {
                    continue;
                }
                let key = (ccx + dx, ccz + dz);
                let Some(chunk) = world.chunks.get(&key) else {
                    continue;
                };
                let ptr = Arc::as_ptr(chunk) as usize;
                let stale = match e.scanned.get(&key) {
                    None => true,
                    Some(&(p, _, _)) if p != ptr => true,
                    Some(&(_, rev, at)) => {
                        ring <= NEAR && rev != world.revision && at.elapsed() >= RESCAN
                    }
                };
                if !stale {
                    continue;
                }
                e.scanned.insert(key, (ptr, world.revision, Instant::now()));
                e.load((key.0 >> 5, (key.1 - 1) >> 5));
                let north = e.tile(key.0, key.1 - 1);
                let Some(tile) = scan_tile(chunk, palette, air, cut, north) else {
                    continue;
                };
                let region = e.region_mut((key.0 >> 5, key.1 >> 5));
                let idx = ((key.1 & 31) * 32 + (key.0 & 31)) as usize;
                let changed = region.tiles[idx]
                    .as_ref()
                    .is_none_or(|t| t.color != tile.color || t.height != tile.height);
                if changed {
                    region.tiles[idx] = Some(Box::new(tile));
                    region.dirty = true;
                }
                if started.elapsed() >= SCAN_BUDGET {
                    return;
                }
            }
        }
    }
}

// --- Hooks: frame, keys, commands --------------------------------------

/// Once a frame: follow the world and the layer, note a death, explore,
/// save.
pub(super) fn frame(game: &mut Game<'_>) {
    if !game.native_window() {
        return;
    }
    with(|s| {
        ensure_settings(s, game);
        let Some(p) = game.native_player() else {
            switch_place(s, game, None);
            return;
        };
        let nether = p.dimension == "minecraft:the_nether";
        let world = world_key(s, game);
        // The map screen was closed by something else (death, a kick).
        if s.screen.is_some() && game.screen != Screen::Advancements {
            s.screen = None;
        }
        // The layer follows the player once it has settled; never while
        // the map is open (the game is paused, the view must stay).
        let cut = cut_now(game, p.x, p.y, p.z, nether);
        let layer = Layer::of(cut);
        let moved = s
            .place
            .as_ref()
            .is_none_or(|pl| pl.world != world || pl.dim != p.dimension);
        let current = s.place.as_ref().map(|pl| pl.layer.clone());
        let follow = if moved {
            true
        } else if current.as_ref() == Some(&layer) {
            s.pending = None;
            false
        } else if s.screen.is_some() {
            false
        } else {
            match &s.pending {
                Some((l, since)) if *l == layer => since.elapsed() >= LAYER_SETTLE,
                _ => {
                    s.pending = Some((layer.clone(), Instant::now()));
                    false
                }
            }
        };
        if follow {
            s.pending = None;
            let now = Place {
                world,
                dim: p.dimension.to_owned(),
                layer: layer.clone(),
            };
            switch_place(s, game, Some(now));
        }
        // Within its layer the cave map is cut at the player's head.
        if s.place.as_ref().is_some_and(|pl| pl.layer == layer) && s.screen.is_none() {
            s.cut = cut;
        }
        let dead = game.screen == Screen::Dead;
        if dead && !s.dead && game.native_on(MINIMAP) {
            s.waypoints.retain(|w| !w.death);
            s.waypoints.push(Waypoint {
                name: tr(s.lang, T::DeathPoint).into(),
                x: p.x.floor() as i32,
                y: p.y.floor() as i32,
                z: p.z.floor() as i32,
                dim: p.dimension.to_owned(),
                color: 0,
                death: true,
            });
            save_waypoints(s, game);
        }
        s.dead = dead;
        // The waypoint key's name prompt: the chat, ready for the name.
        if s.name_prompt {
            s.name_prompt = false;
            if game.native_playing() {
                game.open_chat("/wp add ");
            }
        }
        let in_layer = s.place.as_ref().is_some_and(|pl| pl.layer == Layer::of(s.cut));
        if game.native_on(WORLDMAP) && s.map.explore && in_layer {
            explore(s, game, p.x, p.z);
        }
        if s.last_save.elapsed() >= SAVE_EVERY {
            flush(&mut s.explorer);
            s.last_save = Instant::now();
        }
    });
}

/// The address of the server being joined, for its own map folder.
pub(super) fn connecting(address: &str) {
    with(|s| s.server = address.to_owned());
}

fn open_map(s: &mut State, game: &mut Game<'_>) {
    let Some(p) = game.native_player() else {
        return;
    };
    s.screen = Some(MapScreen {
        cx: p.x,
        cz: p.z,
        zoom: 2.0,
        drag: None,
        held: [false; 4],
        last: Instant::now(),
    });
    game.screen = Screen::Advancements;
    game.capture(false);
}
fn close_map(s: &mut State, game: &mut Game<'_>) {
    s.screen = None;
    flush(&mut s.explorer);
    game.screen = Screen::Playing;
    game.capture(true);
}
fn zoom_map(m: &mut MapScreen, steps: f32, at: (f32, f32), size: (f32, f32)) {
    let old = m.zoom;
    m.zoom = (m.zoom * 1.25f32.powf(steps)).clamp(1.0 / 16.0, 16.0);
    // The block under `at` stays under it.
    let (ox, oy) = (at.0 - size.0 / 2.0, at.1 - size.1 / 2.0);
    m.cx += f64::from(ox / old - ox / m.zoom);
    m.cz += f64::from(oy / old - oy / m.zoom);
}

/// A key went down or up, on any screen.
pub(super) fn key(game: &mut Game<'_>, key: &str, pressed: bool) {
    with(|s| {
        ensure_settings(s, game);
        let open_key = game.native_mod_key(WORLDMAP, KEY_OPEN, "key.keyboard.m");
        let wp_key = game.native_mod_key(MINIMAP, KEY_WAYPOINT, "key.keyboard.b");
        let in_key = game.native_mod_key(MINIMAP, KEY_ZOOM_IN, "key.keyboard.i");
        let out_key = game.native_mod_key(MINIMAP, KEY_ZOOM_OUT, "key.keyboard.o");
        if s.screen.is_some() {
            let size = game.ui_size();
            let cursor = game.cursor;
            let player = game.native_player();
            let Some(m) = s.screen.as_mut() else { return };
            let dir = match key {
                "key.keyboard.w" | "key.keyboard.up" => Some(0),
                "key.keyboard.s" | "key.keyboard.down" => Some(1),
                "key.keyboard.a" | "key.keyboard.left" => Some(2),
                "key.keyboard.d" | "key.keyboard.right" => Some(3),
                _ => None,
            };
            if let Some(d) = dir {
                m.held[d] = pressed;
                return;
            }
            if !pressed {
                return;
            }
            if key == open_key {
                close_map(s, game);
            } else if key == "key.keyboard.space" {
                if let Some(p) = player {
                    m.cx = p.x;
                    m.cz = p.z;
                }
            } else if key == in_key || key == "key.keyboard.equal" {
                zoom_map(m, 2.0, cursor, size);
            } else if key == out_key || key == "key.keyboard.minus" {
                zoom_map(m, -2.0, cursor, size);
            } else if key == wp_key {
                let (bx, bz) = map_block(m, cursor, size);
                let (_, h) = s.explorer.column(bx, bz);
                let y = if h == 0 { 64 } else { i32::from(h) + 1 };
                let dim = s.place.as_ref().map(|pl| pl.dim.clone()).unwrap_or_default();
                add_waypoint(s, game, "", (bx, y, bz), &dim);
            } else if matches!(key, "key.keyboard.delete" | "key.keyboard.backspace" | "key.keyboard.x") {
                // The waypoint under the pointer (its marker's few pixels).
                let (zoom, cx, cz) = (f64::from(m.zoom), m.cx, m.cz);
                let wx = cx + f64::from(cursor.0 - size.0 / 2.0) / zoom;
                let wz = cz + f64::from(cursor.1 - size.1 / 2.0) / zoom;
                let dim = s.place.as_ref().map(|pl| pl.dim.clone()).unwrap_or_default();
                let hit = s
                    .waypoints
                    .iter()
                    .enumerate()
                    .filter(|(_, w)| w.dim == dim)
                    .map(|(i, w)| {
                        let dx = (f64::from(w.x) + 0.5 - wx) * zoom;
                        let dz = (f64::from(w.z) + 0.5 - wz) * zoom;
                        (i, dx.abs().max(dz.abs()))
                    })
                    .filter(|&(_, d)| d <= 6.0)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(i, _)| i);
                if let Some(i) = hit {
                    remove_waypoint(s, game, i);
                }
            }
            return;
        }
        if !pressed || !game.native_playing() {
            return;
        }
        if key == open_key && game.native_on(WORLDMAP) {
            open_map(s, game);
            return;
        }
        if !game.native_on(MINIMAP) {
            return;
        }
        if key == wp_key {
            // On a waypoint: remove it. Elsewhere: ask for the new one's name.
            if let Some(p) = game.native_player() {
                match nearest(s, p.dimension, (p.x, p.y, p.z), 2.0) {
                    Some(i) => remove_waypoint(s, game, i),
                    None => s.name_prompt = true,
                }
            }
        } else if key == in_key || key == out_key {
            let z = s.mini.zoom;
            s.mini.zoom = if key == in_key {
                z.saturating_sub(1)
            } else {
                (z + 1).min(ZOOMS.len() - 1)
            };
            store_settings(s, game);
        }
    });
}

/// `/wp …`: the waypoints of this world.
pub(super) fn command(game: &mut Game<'_>, command: &str, ran: &mut bool) {
    let mut words = command.split_whitespace();
    if !matches!(words.next(), Some("wp" | "waypoint")) || !game.native_on(MINIMAP) {
        return;
    }
    *ran = true;
    let sub = words.next().unwrap_or("list");
    let rest: Vec<&str> = words.collect();
    let rest = rest.join(" ");
    with(|s| {
        ensure_settings(s, game);
        let Some(p) = game.native_player() else {
            return;
        };
        let l = s.lang;
        match sub {
            "add" | "set" => {
                let at = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
                add_waypoint(s, game, &rest, at, p.dimension);
            }
            "del" | "delete" | "remove" | "rm" => {
                let n = rest.trim();
                if n.is_empty() {
                    // The nearest one in this dimension.
                    let here = (p.x, p.y, p.z);
                    match nearest(s, p.dimension, here, f64::INFINITY) {
                        Some(i) => remove_waypoint(s, game, i),
                        None => chat(s, game, tr(l, T::MsgNotFound)),
                    }
                    return;
                }
                let before = s.waypoints.len();
                if let Ok(i) = n.parse::<usize>() {
                    if i >= 1 && i <= s.waypoints.len() {
                        s.waypoints.remove(i - 1);
                    }
                } else {
                    s.waypoints.retain(|w| !w.name.eq_ignore_ascii_case(n));
                }
                let gone = before - s.waypoints.len();
                if gone == 0 {
                    chat(s, game, tr(l, T::MsgNotFound));
                    return;
                }
                save_waypoints(s, game);
                chat(s, game, &fill(tr(l, T::MsgDeleted), &[&gone.to_string()]));
            }
            "rename" | "name" => {
                let mut parts = rest.splitn(2, ' ');
                let index = parts.next().and_then(|n| n.parse::<usize>().ok());
                let name = clean_name(parts.next().unwrap_or(""));
                match index.filter(|&i| i >= 1 && i <= s.waypoints.len()) {
                    Some(i) if !name.is_empty() => {
                        s.waypoints[i - 1].name = name.clone();
                        save_waypoints(s, game);
                        chat(s, game, &fill(tr(l, T::MsgRenamed), &[&name]));
                    }
                    _ => chat(s, game, tr(l, T::MsgHelp)),
                }
            }
            "help" | "?" => chat(s, game, tr(l, T::MsgHelp)),
            "clear" => {
                s.waypoints.clear();
                save_waypoints(s, game);
                chat(s, game, tr(l, T::MsgCleared));
            }
            _ => {
                if s.waypoints.is_empty() {
                    chat(s, game, tr(l, T::MsgNone));
                }
                let lines: Vec<String> = s
                    .waypoints
                    .iter()
                    .enumerate()
                    .map(|(i, w)| {
                        let d = ((f64::from(w.x) - p.x).powi(2) + (f64::from(w.z) - p.z).powi(2)).sqrt();
                        let here = if w.dim == p.dimension {
                            format!("{} m", d as i64)
                        } else {
                            tr(l, T::OtherDim).into()
                        };
                        format!("§7{}.§r §e{}§r  {} {} {}  §7({here})", i + 1, w.name, w.x, w.y, w.z)
                    })
                    .collect();
                for line in lines {
                    game.native_chat(&line);
                }
            }
        }
    });
}

// --- Minimap -----------------------------------------------------------

/// The minimap Horizon draws for us.
pub(super) fn minimap(game: &Game<'_>, minimap: &mut Option<NativeMinimap>) {
    if minimap.is_some() || !game.native_on(MINIMAP) {
        return;
    }
    let m = with(|s| {
        ensure_settings(s, game);
        s.mini
    });
    *minimap = Some(NativeMinimap {
        corner: CORNERS[m.corner],
        size: SIZES[m.size],
        blocks_per_pixel: ZOOMS[m.zoom],
        rotate: m.rotate,
        round: m.round,
    });
}

fn argb(c: u32) -> u32 {
    let [a, r, g, b] = c.to_be_bytes();
    rgba(r, g, b, a)
}
fn facing(lang: usize, yaw: f32) -> &'static str {
    // 0 faces south, 90 west, 180 north, 270 east.
    let i = ((yaw.rem_euclid(360.0) + 22.5) / 45.0) as usize % 8;
    let dirs = [
        T::South,
        T::SouthWest,
        T::West,
        T::NorthWest,
        T::North,
        T::NorthEast,
        T::East,
        T::SouthEast,
    ];
    tr(lang, dirs[i])
}
/// A small diamond.
fn marker(batch: &mut HudBatch, game: &Game<'_>, x: f32, y: f32, r: f32, color: u32) {
    let atlas = &game.gui.atlas;
    batch.fill_quad(
        atlas,
        [[x, y - r - 1.0], [x + r + 1.0, y], [x, y + r + 1.0], [x - r - 1.0, y]],
        argb(0xFF000000),
    );
    batch.fill_quad(atlas, [[x, y - r], [x + r, y], [x, y + r], [x - r, y]], argb(color));
}

/// The waypoints in the world: a marker and `name · 123 m` over each one
/// on screen, projected from the last frame's camera as the locator bar
/// does (`game/locator_bar.rs`).
fn world_markers(s: &State, game: &Game<'_>, batch: &mut HudBatch, p: &NativePlayer) {
    let Some((cam, yaw, pitch, fov)) = game.locator_camera else {
        return;
    };
    let (w, h) = (batch.width, batch.height);
    let (sy, cy) = f64::from(yaw.to_radians()).sin_cos();
    let (sp, cp) = f64::from(pitch.to_radians()).sin_cos();
    let forward = [-sy * cp, -sp, cy * cp];
    let up = [-sy * sp, cp, cy * sp];
    let right = [-cy, 0.0, -sy];
    let focal = 1.0 / f64::from((fov / 2.0).to_radians()).tan();
    let aspect = f64::from(w / h.max(1.0));
    let atlas = &game.gui.atlas;
    for wp in s.waypoints.iter().filter(|wp| wp.dim == p.dimension) {
        let target = [f64::from(wp.x) + 0.5, f64::from(wp.y) + 1.0, f64::from(wp.z) + 0.5];
        let o = [target[0] - cam[0], target[1] - cam[1], target[2] - cam[2]];
        let dot = |a: [f64; 3]| a[0] * o[0] + a[1] * o[1] + a[2] * o[2];
        let depth = dot(forward);
        if depth < 0.1 {
            continue;
        }
        let nx = dot(right) * focal / aspect / depth;
        let ny = dot(up) * focal / depth;
        if nx.abs() > 1.0 || ny.abs() > 1.0 {
            continue;
        }
        let x = ((nx + 1.0) / 2.0) as f32 * w;
        let y = ((1.0 - ny) / 2.0) as f32 * h;
        let dist = ((target[0] - p.x).powi(2)
            + (f64::from(wp.y) - p.y).powi(2)
            + (target[2] - p.z).powi(2))
        .sqrt();
        let color = if wp.death { 0xFFAAAAAA } else { COLORS[wp.color] };
        marker(batch, game, x, y, 3.0, color);
        let label = format!("{} · {} m", wp.name, dist.round() as i64);
        let tw = HudBatch::text_width(atlas, &label);
        let (lx, ly) = ((x - tw / 2.0).round(), (y - 16.0).round());
        batch.fill(atlas, lx - 2.0, ly - 1.0, tw + 3.0, 10.0, argb(0x60000000));
        batch.text(atlas, &label, lx, ly, argb(color), true);
    }
}

/// The HUD after Horizon's minimap: its info box under it, and the
/// waypoints on it.
pub(super) fn hud(game: &Game<'_>, batch: &mut HudBatch, corners: &mut Corners) {
    if !game.native_on(MINIMAP) {
        return;
    }
    let Some(p) = game.native_player() else {
        return;
    };
    with(|s| {
        ensure_settings(s, game);
        let m = s.mini;
        if m.world {
            world_markers(s, game, batch, &p);
        }
        let corner = CORNERS[m.corner];
        // Horizon leaves the top left corner to F3's screen.
        if game.debug && corner == Corner::TopLeft {
            return;
        }
        let size = SIZES[m.size];
        let atlas = &game.gui.atlas;
        let lines: Vec<String> = if m.coords {
            vec![
                format!("{} {} {}", p.x.floor() as i64, p.y.floor() as i64, p.z.floor() as i64),
                facing(s.lang, p.yaw).to_owned(),
            ]
        } else {
            Vec::new()
        };
        let h = if lines.is_empty() { 0.0 } else { 10.0 * lines.len() as f32 + 2.0 };
        let (x, y) = corners.place(corner, size, h);
        let map_y = if corner.top() { y - MARGIN - size } else { y + h + MARGIN };
        if !lines.is_empty() {
            batch.fill(atlas, x, y, size, h, argb(0x78000000));
            for (i, l) in lines.iter().enumerate() {
                let w = HudBatch::text_width(atlas, l);
                let tx = (x + (size - w) / 2.0).round();
                batch.text(atlas, l, tx, y + 2.0 + 10.0 * i as f32, u32::MAX, true);
            }
        }
        if !m.waypoints {
            return;
        }
        // The minimap's projection (`minimap::View::axes`).
        let (cx, cy) = (x + size / 2.0, map_y + size / 2.0);
        let px_per_block = size / IMAGE / ZOOMS[m.zoom];
        let (r, f) = if m.rotate {
            let (sn, c) = p.yaw.to_radians().sin_cos();
            ([-c, -sn], [-sn, c])
        } else {
            ([1.0, 0.0], [0.0, -1.0])
        };
        let half = size / 2.0 - 4.0;
        for w in s.waypoints.iter().filter(|w| w.dim == p.dimension) {
            let dx = (f64::from(w.x) + 0.5 - p.x) as f32;
            let dz = (f64::from(w.z) + 0.5 - p.z) as f32;
            let mut u = (dx * r[0] + dz * r[1]) * px_per_block;
            let mut v = -(dx * f[0] + dz * f[1]) * px_per_block;
            let d = if m.round { (u * u + v * v).sqrt() } else { u.abs().max(v.abs()) };
            let edge = d > half;
            if edge {
                u *= half / d;
                v *= half / d;
            }
            let color = if w.death { 0xFFAAAAAA } else { COLORS[w.color] };
            marker(batch, game, cx + u, cy + v, if edge { 1.5 } else { 2.5 }, color);
        }
    });
}

// --- World map screen --------------------------------------------------

impl Game<'_> {
    /// The GUI's size in GUI pixels.
    fn ui_size(&self) -> (f32, f32) {
        let size = self
            .renderer
            .as_ref()
            .map_or([854, 480], |r| [r.config.width, r.config.height]);
        let b = HudBatch::new(size);
        (b.width, b.height)
    }
}
/// The block at GUI position `at`.
fn map_block(m: &MapScreen, at: (f32, f32), size: (f32, f32)) -> (i32, i32) {
    let x = m.cx + f64::from((at.0 - size.0 / 2.0) / m.zoom);
    let z = m.cz + f64::from((at.1 - size.1 / 2.0) / m.zoom);
    (x.floor() as i32, z.floor() as i32)
}
/// `MapColor.getColorFromPackedId` for each colour byte, as HUD colours.
fn lut() -> [u32; 256] {
    let mut out = [0u32; 256];
    for (i, o) in out.iter_mut().enumerate() {
        let id = i >> 2;
        if id == 0 || id >= MAP_COLORS.len() {
            continue;
        }
        let modifier = [180u32, 220, 255, 135][i & 3];
        let col = MAP_COLORS[id];
        let ch = |s: u32| ((col >> s & 255) * modifier / 255) as u8;
        *o = rgba(ch(16), ch(8), ch(0), 255);
    }
    out
}
fn dim_name(lang: usize, dim: &str) -> String {
    match dim {
        "minecraft:overworld" => tr(lang, T::Overworld).into(),
        "minecraft:the_nether" => tr(lang, T::Nether).into(),
        "minecraft:the_end" => tr(lang, T::End).into(),
        other => other.into(),
    }
}

/// Draws the map instead of the Advancements screen while it is open.
pub(super) fn draw(game: &mut Game<'_>, batch: &mut HudBatch) -> std::ops::ControlFlow<()> {
    with(|s| {
        let Some(m) = s.screen.as_mut() else {
            return std::ops::ControlFlow::Continue(());
        };
        let l = s.lang;
        let (w, h) = (batch.width, batch.height);
        // Keys held pan the map.
        let dt = m.last.elapsed().as_secs_f32().min(0.1);
        m.last = Instant::now();
        let speed = f64::from(400.0 * dt / m.zoom);
        m.cz += speed * (f64::from(u8::from(m.held[1])) - f64::from(u8::from(m.held[0])));
        m.cx += speed * (f64::from(u8::from(m.held[3])) - f64::from(u8::from(m.held[2])));
        let (zoom, cx, cz) = (m.zoom, m.cx, m.cz);
        let cursor = game.cursor;
        let hovered = map_block(m, cursor, (w, h));

        let atlas = &game.gui.atlas;
        batch.fill(atlas, 0.0, 0.0, w, h, argb(0xFF101418));

        // The regions in view, read before drawing.
        let left = cx - f64::from(w / 2.0 / zoom);
        let top = cz - f64::from(h / 2.0 / zoom);
        let right = cx + f64::from(w / 2.0 / zoom);
        let bottom = cz + f64::from(h / 2.0 / zoom);
        let e = &mut s.explorer;
        for rz in ((top.floor() as i32) >> 9)..=((bottom.floor() as i32) >> 9) {
            for rx in ((left.floor() as i32) >> 9)..=((right.floor() as i32) >> 9) {
                let _ = e.region((rx, rz));
            }
        }
        let colors = lut();
        let cell = zoom.max(w / MAX_COLUMNS);
        let step = f64::from(cell / zoom);
        let cols = (w / cell).ceil() as i32 + 1;
        let rows = (h / cell).ceil() as i32 + 1;
        // Cells line up with blocks when a block covers several pixels.
        let aligned = cell <= zoom;
        let (base_x, base_z) = if aligned { (left.floor(), top.floor()) } else { (left, top) };
        let (ox, oz) = if aligned {
            (((base_x - left) * f64::from(zoom)) as f32, ((base_z - top) * f64::from(zoom)) as f32)
        } else {
            (0.0, 0.0)
        };
        for j in 0..rows {
            let wz = (base_z + (f64::from(j) + 0.5) * step).floor() as i32;
            let y = oz + j as f32 * cell;
            let mut run: Option<(i32, u32)> = None;
            for i in 0..=cols {
                let c = if i < cols {
                    let wx = (base_x + (f64::from(i) + 0.5) * step).floor() as i32;
                    colors[usize::from(e.column(wx, wz).0)]
                } else {
                    0
                };
                match run {
                    Some((_, rc)) if rc == c => {}
                    Some((start, rc)) => {
                        if rc != 0 {
                            let x = ox + start as f32 * cell;
                            batch.fill(atlas, x, y, (i - start) as f32 * cell, cell, rc);
                        }
                        run = Some((i, c));
                    }
                    None => run = Some((i, c)),
                }
            }
        }
        // Chunk grid.
        if s.map.grid && zoom >= 2.0 {
            let line = argb(0x30FFFFFF);
            let mut gx = (left / 16.0).floor() * 16.0;
            while gx <= right {
                let x = ((gx - left) * f64::from(zoom)) as f32;
                batch.fill(atlas, x, 0.0, 1.0, h, line);
                gx += 16.0;
            }
            let mut gz = (top / 16.0).floor() * 16.0;
            while gz <= bottom {
                let y = ((gz - top) * f64::from(zoom)) as f32;
                batch.fill(atlas, 0.0, y, w, 1.0, line);
                gz += 16.0;
            }
        }
        let to_screen = |x: f64, z: f64| -> (f32, f32) {
            (((x - left) * f64::from(zoom)) as f32, ((z - top) * f64::from(zoom)) as f32)
        };
        let dim = s.place.as_ref().map(|pl| pl.dim.clone()).unwrap_or_default();
        // Waypoints.
        let mut label: Option<String> = None;
        if s.map.waypoints {
            for wp in s.waypoints.iter().filter(|wp| wp.dim == dim) {
                let (sx, sy) = to_screen(f64::from(wp.x) + 0.5, f64::from(wp.z) + 0.5);
                if sx < -10.0 || sy < -10.0 || sx > w + 10.0 || sy > h + 10.0 {
                    continue;
                }
                let color = if wp.death { 0xFFAAAAAA } else { COLORS[wp.color] };
                marker(batch, game, sx, sy, 3.0, color);
                let near = (cursor.0 - sx).abs() < 6.0 && (cursor.1 - sy).abs() < 6.0;
                if near {
                    label = Some(format!("{}  ({} {} {})", wp.name, wp.x, wp.y, wp.z));
                }
                if zoom >= 0.5 || near {
                    let tw = HudBatch::text_width(atlas, &wp.name);
                    batch.text(atlas, &wp.name, (sx - tw / 2.0).round(), sy + 5.0, argb(color), true);
                }
            }
        }
        // The player's arrow (`extras_hud::draw_minimap`).
        if let Some(p) = game.native_player() {
            let (px, py) = to_screen(p.x, p.z);
            if let Some(r) = atlas.sprites.get("map/decorations/player").copied() {
                let (sn, c) = (p.yaw + 360.0).to_radians().sin_cos();
                let q = |dx: f32, dy: f32| [px + dx * c - dy * sn, py + dx * sn + dy * c];
                let a = 5.0;
                batch.quad(
                    [q(-a, a), q(a, a), q(a, -a), q(-a, -a)],
                    [[r.u0, r.v0], [r.u1, r.v0], [r.u1, r.v1], [r.u0, r.v1]],
                    u32::MAX,
                );
            } else {
                marker(batch, game, px, py, 3.0, 0xFFFFFFFF);
            }
        }
        // Bars: the title names the dimension and the layer shown.
        batch.fill(atlas, 0.0, 0.0, w, 14.0, argb(0xA0000000));
        let layer = match (&s.place, s.cut) {
            (Some(pl), Some(head)) if matches!(pl.layer, Layer::Cave(_)) => {
                fill(tr(l, T::Cave), &[&head.to_string()])
            }
            _ => tr(l, T::Surface).into(),
        };
        let title = format!("{} – {} – {}", tr(l, T::WorldTitle), dim_name(l, &dim), layer);
        let tw = HudBatch::text_width(atlas, &title);
        batch.text(atlas, &title, ((w - tw) / 2.0).round(), 3.0, u32::MAX, true);
        batch.fill(atlas, 0.0, h - 26.0, w, 26.0, argb(0xA0000000));
        let (_, height) = e.column(hovered.0, hovered.1);
        let at = if height == 0 {
            format!("X {}  Z {}", hovered.0, hovered.1)
        } else {
            format!("X {}  Y {}  Z {}", hovered.0, height, hovered.1)
        };
        let info = label.unwrap_or(at);
        batch.text(atlas, &info, 4.0, h - 23.0, u32::MAX, true);
        let scale = if zoom >= 1.0 {
            fill(tr(l, T::PxPerBlock), &[&((zoom * 10.0).round() / 10.0).to_string()])
        } else {
            fill(tr(l, T::BlocksPerPx), &[&((10.0 / zoom).round() / 10.0).to_string()])
        };
        let sw = HudBatch::text_width(atlas, &scale);
        batch.text(atlas, &scale, w - sw - 4.0, h - 23.0, argb(0xFFAAAAAA), true);
        batch.text(atlas, tr(l, T::Hint), 4.0, h - 12.0, argb(0xFFAAAAAA), true);
        std::ops::ControlFlow::Break(())
    })
}

/// The left button on the map: drag it.
pub(super) fn mouse(game: &mut Game<'_>, pressed: bool) -> std::ops::ControlFlow<()> {
    with(|s| {
        let Some(m) = s.screen.as_mut() else {
            return std::ops::ControlFlow::Continue(());
        };
        m.drag = pressed.then_some(game.cursor);
        std::ops::ControlFlow::Break(())
    })
}
pub(super) fn drag(game: &mut Game<'_>) -> std::ops::ControlFlow<()> {
    with(|s| {
        let Some(m) = s.screen.as_mut() else {
            return std::ops::ControlFlow::Continue(());
        };
        if let Some(last) = m.drag {
            let now = game.cursor;
            m.cx -= f64::from((now.0 - last.0) / m.zoom);
            m.cz -= f64::from((now.1 - last.1) / m.zoom);
            m.drag = Some(now);
        }
        std::ops::ControlFlow::Break(())
    })
}
pub(super) fn scroll(game: &mut Game<'_>, y: f32) -> std::ops::ControlFlow<()> {
    let size = game.ui_size();
    let cursor = game.cursor;
    with(|s| {
        let Some(m) = s.screen.as_mut() else {
            return std::ops::ControlFlow::Continue(());
        };
        if y != 0.0 {
            zoom_map(m, y.signum(), cursor, size);
        }
        std::ops::ControlFlow::Break(())
    })
}
/// Esc (or the Advancements key) on the map closes it.
pub(super) fn close(game: &mut Game<'_>) -> std::ops::ControlFlow<()> {
    with(|s| {
        if s.screen.is_none() {
            return std::ops::ControlFlow::Continue(());
        }
        close_map(s, game);
        std::ops::ControlFlow::Break(())
    })
}
