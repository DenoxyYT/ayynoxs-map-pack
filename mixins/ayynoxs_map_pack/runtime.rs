// ayynoxs map pack: a minimap with waypoints over Horizon's own minimap,
// and a world map of every chunk the player has seen, kept on disk per
// world, dimension and layer. Both share the waypoints and the language,
// and players on the same server can share both (`sync`).
//
// The window's state lives here in a thread local, never in `Game`'s
// fields. The relay that a Horizon host runs for its guests keeps its own
// state behind a mutex, as it may run on the server's thread.
use super::{
    Game, Screen,
    native_hooks::{NativeMinimap, NativeMod, NativePlayer},
};
use horizon_client::{
    biome::Biomes,
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
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub(crate) const MINIMAP: &str = "ayynoxs_minimap";
pub(crate) const WORLDMAP: &str = "ayynoxs_worldmap";
pub(crate) const VERSION: &str = "0.4.0";

const KEY_WAYPOINT: &str = "waypoint";
const KEY_REMOVE: &str = "remove";
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
/// The world map's columns at most; zoomed out, a cell averages its blocks.
const MAX_COLUMNS: f32 = 320.0;
/// A waypoint name's characters at most.
const NAME_MAX: usize = 32;
/// In the world, the waypoint the crosshair is this close to (GUI pixels)
/// is the one the remove key takes.
const AIM: f32 = 24.0;

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
    KeyRemove,
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
    InWorld,
    On,
    Off,
    Explore,
    Grid,
    MapWaypoints,
    Share,
    WaypointN,
    DeathPoint,
    Prefix,
    MsgSet,
    MsgDeleted,
    MsgRemoved,
    MsgRenamed,
    MsgCleared,
    MsgNone,
    MsgNotFound,
    MsgShared,
    MsgReceived,
    MsgNoSync,
    MsgSentTo,
    MsgHelp,
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
    NameTitle,
    RenameTitle,
    NameHint,
    Synced,
}

/// `t` in language `lang` (an index of `LANGS`).
fn tr(lang: usize, t: T) -> &'static str {
    let row: [&str; 5] = match t {
        T::WorldTitle => ["ayynoxs world map", "ayynoxs weltkarte", "ayynoxs carte du monde", "ayynoxs mappa del mondo", "ayynoxs карта мира"],
        T::MiniDesc => [
            "Minimap with coordinates, direction, waypoints and a death point. /wp help",
            "Minimap mit Koordinaten, Himmelsrichtung, Wegpunkten und Todespunkt. /wp help",
            "Minicarte avec coordonnées, direction, points de passage et point de mort. /wp help",
            "Minimappa con coordinate, direzione, waypoint e punto di morte. /wp help",
            "Миникарта с координатами, направлением, метками и точкой смерти. /wp help",
        ],
        T::WorldDesc => [
            "Full-screen map of every chunk you explored, saved per world and layer, shared with your friends.",
            "Vollbild-Karte aller erkundeten Chunks, pro Welt und Ebene gespeichert und mit Freunden geteilt.",
            "Carte plein écran de tous les chunks explorés, par monde et par couche, partagée avec tes amis.",
            "Mappa a schermo intero dei chunk esplorati, per mondo e livello, condivisa con gli amici.",
            "Полноэкранная карта исследованных чанков, по мирам и слоям, общая с друзьями.",
        ],
        T::KeyWaypoint => ["Set waypoint", "Wegpunkt setzen", "Placer un point", "Imposta waypoint", "Поставить метку"],
        T::KeyRemove => ["Remove waypoint", "Wegpunkt löschen", "Supprimer un point", "Elimina waypoint", "Удалить метку"],
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
        T::InWorld => ["Waypoints in the world", "Wegpunkte in der Welt", "Points dans le monde", "Waypoint nel mondo", "Метки в мире"],
        T::On => ["On", "An", "Activé", "Attivo", "Вкл"],
        T::Off => ["Off", "Aus", "Désactivé", "Disattivo", "Выкл"],
        T::Explore => ["Explore & save", "Erkunden & speichern", "Explorer et enregistrer", "Esplora e salva", "Исследовать и сохранять"],
        T::Grid => ["Chunk grid", "Chunk-Raster", "Grille des chunks", "Griglia dei chunk", "Сетка чанков"],
        T::MapWaypoints => ["Waypoints on the map", "Wegpunkte auf der Karte", "Points sur la carte", "Waypoint sulla mappa", "Метки на карте"],
        T::Share => [
            "Share map & waypoints in multiplayer",
            "Karte & Wegpunkte im Multiplayer teilen",
            "Partager carte et points en multijoueur",
            "Condividi mappa e waypoint in multiplayer",
            "Делиться картой и метками в мультиплеере",
        ],
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
        T::MsgCleared => [
            "All waypoints of this world deleted",
            "Alle Wegpunkte dieser Welt gelöscht",
            "Tous les points de ce monde ont été supprimés",
            "Tutti i waypoint di questo mondo eliminati",
            "Все метки этого мира удалены",
        ],
        T::MsgNone => [
            "No waypoints yet. Press B to set one.",
            "Keine Wegpunkte. Drück B, um einen zu setzen.",
            "Aucun point. Appuie sur B pour en placer un.",
            "Nessun waypoint. Premi B per impostarne uno.",
            "Меток нет. Нажми B, чтобы поставить.",
        ],
        T::MsgNotFound => [
            "No waypoint found. /wp list shows them all.",
            "Kein Wegpunkt gefunden. /wp list zeigt alle.",
            "Aucun point trouvé. /wp list les affiche tous.",
            "Nessun waypoint trovato. /wp list li mostra tutti.",
            "Метка не найдена. /wp list покажет все.",
        ],
        T::MsgShared => [
            "Waypoint §e{0}§r shared with the other players",
            "Wegpunkt §e{0}§r mit den anderen Spielern geteilt",
            "Point §e{0}§r partagé avec les autres joueurs",
            "Waypoint §e{0}§r condiviso con gli altri giocatori",
            "Метка §e{0}§r отправлена другим игрокам",
        ],
        T::MsgReceived => [
            "New shared waypoint: §e{0}§r at {1}",
            "Neuer geteilter Wegpunkt: §e{0}§r bei {1}",
            "Nouveau point partagé : §e{0}§r en {1}",
            "Nuovo waypoint condiviso: §e{0}§r a {1}",
            "Новая общая метка: §e{0}§r, {1}",
        ],
        T::MsgSentTo => [
            "Waypoint §e{0}§r sent to {1}",
            "Wegpunkt §e{0}§r an {1} geschickt",
            "Point §e{0}§r envoyé à {1}",
            "Waypoint §e{0}§r inviato a {1}",
            "Метка §e{0}§r отправлена игроку {1}",
        ],
        T::MsgNoSync => [
            "Send it to a player with /wp share <nr> <player>: that works on every server.",
            "Schick ihn einem Spieler mit /wp share <Nr> <Spieler>: das geht auf jedem Server.",
            "Envoie-le à un joueur avec /wp share <n°> <joueur> : ça marche sur tous les serveurs.",
            "Invialo a un giocatore con /wp share <nr> <giocatore>: funziona su ogni server.",
            "Отправь её игроку: /wp share <№> <игрок> — работает на любом сервере.",
        ],
        T::MsgHelp => [
            "B: set or rename a waypoint (a window asks the name) · Backspace: remove the one you aim at · /wp add <name> · /wp del [name|nr] · /wp rename <nr> <name> · /wp share <nr> [player] · /wp list · /wp clear",
            "B: Wegpunkt setzen oder umbenennen (ein Fenster fragt den Namen) · Rücktaste: anvisierten löschen · /wp add <Name> · /wp del [Name|Nr] · /wp rename <Nr> <Name> · /wp share <Nr> [Spieler] · /wp list · /wp clear",
            "B : placer ou renommer un point (une fenêtre demande le nom) · Retour arrière : supprimer celui visé · /wp add <nom> · /wp del [nom|n°] · /wp rename <n°> <nom> · /wp share <n°> [joueur] · /wp list · /wp clear",
            "B: imposta o rinomina un waypoint (una finestra chiede il nome) · Backspace: elimina quello mirato · /wp add <nome> · /wp del [nome|nr] · /wp rename <nr> <nome> · /wp share <nr> [giocatore] · /wp list · /wp clear",
            "B: поставить или переименовать метку (окно спросит имя) · Backspace: удалить метку в прицеле · /wp add <имя> · /wp del [имя|№] · /wp rename <№> <имя> · /wp share <№> [игрок] · /wp list · /wp clear",
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
            "Drag/WASD: move · Wheel: zoom · Space: to you · B: waypoint · Backspace: remove · M/Esc: close",
            "Ziehen/WASD: bewegen · Mausrad: Zoom · Leertaste: zu dir · B: Wegpunkt · Rücktaste: löschen · M/Esc: schließen",
            "Glisser/ZQSD : déplacer · Molette : zoom · Espace : vers toi · B : point · Retour : supprimer · M/Échap : fermer",
            "Trascina/WASD: muovi · Rotella: zoom · Spazio: da te · B: waypoint · Backspace: elimina · M/Esc: chiudi",
            "Тащить/WASD: двигать · Колесо: масштаб · Пробел: к себе · B: метка · Backspace: удалить · M/Esc: закрыть",
        ],
        T::PxPerBlock => ["{0} px/block", "{0} px/Block", "{0} px/bloc", "{0} px/blocco", "{0} px/блок"],
        T::BlocksPerPx => ["1 px = {0} blocks", "1 px = {0} Blöcke", "1 px = {0} blocs", "1 px = {0} blocchi", "1 px = {0} блоков"],
        T::NameTitle => ["New waypoint", "Neuer Wegpunkt", "Nouveau point", "Nuovo waypoint", "Новая метка"],
        T::RenameTitle => ["Rename waypoint", "Wegpunkt umbenennen", "Renommer le point", "Rinomina waypoint", "Переименовать метку"],
        T::NameHint => [
            "Enter: save · Esc: cancel",
            "Enter: speichern · Esc: abbrechen",
            "Entrée : enregistrer · Échap : annuler",
            "Invio: salva · Esc: annulla",
            "Enter: сохранить · Esc: отмена",
        ],
        T::Synced => ["shared", "geteilt", "partagée", "condivisa", "общая"],
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
    /// Explored chunks and new waypoints go to the other players.
    share: bool,
}
impl Default for MapSettings {
    fn default() -> Self {
        Self {
            explore: true,
            grid: false,
            waypoints: true,
            share: true,
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

/// What `depth` holds for a wall the cave map cuts through.
const WALL_DEPTH: u8 = 255;
/// One chunk of the map: per column its colour (biome-tinted, water over
/// its floor; all zero is unknown), height and water depth.
#[derive(Clone)]
struct Tile {
    rgb: [u8; 768],
    height: [i16; 256],
    depth: [u8; 256],
}
impl Tile {
    fn empty() -> Self {
        Self {
            rgb: [0; 768],
            height: [MIN_Y as i16; 256],
            depth: [0; 256],
        }
    }
    const BYTES: usize = 768 + 512 + 256;
    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.rgb);
        for h in self.height {
            out.extend_from_slice(&h.to_le_bytes());
        }
        out.extend_from_slice(&self.depth);
    }
    fn read(b: &[u8]) -> Option<Self> {
        if b.len() < Self::BYTES {
            return None;
        }
        let mut t = Self::empty();
        t.rgb.copy_from_slice(&b[..768]);
        for i in 0..256 {
            t.height[i] = i16::from_le_bytes([b[768 + i * 2], b[769 + i * 2]]);
        }
        t.depth.copy_from_slice(&b[1280..1536]);
        Some(t)
    }
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

/// What a scan needs of each block state: map colour, flags and tint.
struct Palette {
    color: Vec<u8>,
    flags: Vec<u8>,
    tint: Vec<u8>,
}
const WATER_FLAG: u8 = 1;
const LAVA_FLAG: u8 = 2;
const FLUID: u8 = 3;
const AIR: u8 = 4;
const STURDY_TOP: u8 = 8;
/// `MapColor.WATER`.
const WATER: u8 = 12;
const LAVA: u8 = 4;
const TINT_NONE: u8 = 0;
const TINT_GRASS: u8 = 1;
const TINT_FOLIAGE: u8 = 2;
const TINT_WATER: u8 = 3;
const TINT_SPRUCE: u8 = 4;
const TINT_BIRCH: u8 = 5;
impl Palette {
    fn new(facts: &BlockFacts) -> Self {
        let n = facts.states.len();
        let (mut color, mut flags, mut tint) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
        for f in &facts.states {
            color.push(f.map_color);
            let mut b = match f.fluid {
                Fluid::None => 0,
                Fluid::Water => WATER_FLAG,
                Fluid::Lava => LAVA_FLAG,
            };
            let name: &str = &f.name;
            if matches!(name, "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air") {
                b |= AIR;
            }
            if f.sturdy_full >> 1 & 1 != 0 {
                b |= STURDY_TOP;
            }
            flags.push(b);
            // The blocks the game tints by biome (`BlockColors`).
            let t = match name {
                "minecraft:grass_block" | "minecraft:short_grass" | "minecraft:tall_grass"
                | "minecraft:fern" | "minecraft:large_fern" | "minecraft:sugar_cane" => TINT_GRASS,
                "minecraft:spruce_leaves" => TINT_SPRUCE,
                "minecraft:birch_leaves" => TINT_BIRCH,
                "minecraft:oak_leaves" | "minecraft:jungle_leaves" | "minecraft:acacia_leaves"
                | "minecraft:dark_oak_leaves" | "minecraft:mangrove_leaves" | "minecraft:vine" => TINT_FOLIAGE,
                "minecraft:water" | "minecraft:bubble_column" => TINT_WATER,
                _ => TINT_NONE,
            };
            tint.push(t);
        }
        Self { color, flags, tint }
    }
    #[inline]
    fn get(&self, id: u32) -> (u8, u8) {
        let i = id as usize;
        (
            self.color.get(i).copied().unwrap_or(0),
            self.flags.get(i).copied().unwrap_or(AIR),
        )
    }
    #[inline]
    fn tint(&self, id: u32) -> u8 {
        self.tint.get(id as usize).copied().unwrap_or(TINT_NONE)
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
    /// As the sync messages carry it.
    fn wire(&self) -> i32 {
        match self {
            Layer::Surface => i32::MIN,
            Layer::Cave(band) => *band,
        }
    }
    fn from_wire(v: i32) -> Self {
        if v == i32::MIN { Layer::Surface } else { Layer::Cave(v) }
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
    /// Counts each change of a tile, for the map's cache.
    revision: u64,
    /// Chunks scanned or changed since they were last shared.
    outbox: Vec<(i32, i32)>,
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

/// The map's cells as last drawn, and what they were drawn for.
#[derive(Default)]
struct MapCache {
    key: Option<(u64, u64, u32, u32, u32, u64)>,
    cells: Vec<u32>,
}

/// The name window: for a new waypoint at `at`, or to rename waypoint
/// `rename`.
struct NameDialog {
    text: String,
    at: (i32, i32, i32),
    dim: String,
    rename: Option<usize>,
}
/// What the name window's keys asked for, done on the next frame (the
/// screen hooks may not change the game).
#[derive(Clone, Copy, PartialEq, Eq)]
enum DialogEnd {
    Save,
    Cancel,
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
    /// The waypoint key: the name window opens on the next frame, so the
    /// key's own character doesn't land in it.
    open_dialog: bool,
    dialog: Option<NameDialog>,
    dialog_end: Option<DialogEnd>,
    /// The scan's palette, for the world's block states.
    palette: Option<Palette>,
    explorer: Explorer,
    screen: Option<MapScreen>,
    cache: MapCache,
    last_save: Instant,
    sync: SyncState,
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
            open_dialog: false,
            dialog: None,
            dialog_end: None,
            palette: None,
            explorer: Explorer::default(),
            screen: None,
            cache: MapCache::default(),
            last_save: Instant::now(),
            sync: SyncState::default(),
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
        share: flag(game, WORLDMAP, "share", m.share),
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
        ("share", w.share),
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
            [KEY_REMOVE.into(), tr(lang, T::KeyRemove).into(), "key.keyboard.backspace".into()],
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
            buttons.push(format!("{}: {}", tr(l, T::Share), on_off(l, m.share)));
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
                3 => m.share = !m.share,
                4 => s.lang = next_lang,
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
        .filter(|c| !c.is_control() && *c != '\t')
        .take(NAME_MAX)
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
fn rename_waypoint(s: &mut State, game: &mut Game<'_>, i: usize, name: &str) {
    let name = clean_name(name);
    if name.is_empty() || i >= s.waypoints.len() {
        return;
    }
    s.waypoints[i].name = name.clone();
    save_waypoints(s, game);
    chat(s, game, &fill(tr(s.lang, T::MsgRenamed), &[&name]));
}
/// A new waypoint at `at`; on an existing one (not a death point) it
/// renames that one instead of stacking a second one there. Shared with
/// the other players when sharing is on.
fn add_waypoint(s: &mut State, game: &mut Game<'_>, name: &str, at: (i32, i32, i32), dim: &str) {
    let spot = (f64::from(at.0) + 0.5, f64::from(at.1), f64::from(at.2) + 0.5);
    if let Some(i) = nearest(s, dim, spot, 1.5).filter(|&i| !s.waypoints[i].death) {
        rename_waypoint(s, game, i, name);
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
    let w = Waypoint {
        name,
        x: at.0,
        y: at.1,
        z: at.2,
        dim: dim.to_owned(),
        color,
        death: false,
    };
    if s.map.share && s.sync.ready {
        s.sync.out.push(encode_waypoint(&w));
    }
    s.waypoints.push(w);
    save_waypoints(s, game);
}

fn region_path(dir: &PathBuf, (rx, rz): (i32, i32)) -> PathBuf {
    dir.join(format!("r.{rx}.{rz}.amr"))
}
/// `AMR2`: a count, then per chunk its index and the tile. `AMR1` files
/// (0.1 to 0.3) hold map colour ids; they read as unshaded colours.
fn read_region(path: &PathBuf) -> Option<Region> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() < 6 {
        return None;
    }
    let v1 = &bytes[..4] == b"AMR1";
    if !v1 && &bytes[..4] != b"AMR2" {
        return None;
    }
    let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
    let mut region = Region::new();
    let mut at = 6;
    let entry = if v1 { 2 + 256 + 512 } else { 2 + Tile::BYTES };
    for _ in 0..count {
        let e = bytes.get(at..at + entry)?;
        let idx = u16::from_le_bytes([e[0], e[1]]) as usize;
        if idx >= 1024 {
            return None;
        }
        let tile = if v1 {
            let mut t = Tile::empty();
            for i in 0..256 {
                let b = e[2 + i];
                let id = usize::from(b >> 2);
                if id != 0 && id < MAP_COLORS.len() {
                    let col = MAP_COLORS[id];
                    t.rgb[i * 3] = (col >> 16) as u8;
                    t.rgb[i * 3 + 1] = (col >> 8) as u8;
                    t.rgb[i * 3 + 2] = col as u8;
                    if b & 3 == 3 {
                        t.depth[i] = WALL_DEPTH;
                    }
                }
                t.height[i] = i16::from_le_bytes([e[258 + i * 2], e[259 + i * 2]]);
            }
            t
        } else {
            Tile::read(&e[2..])?
        };
        region.tiles[idx] = Some(Box::new(tile));
        at += entry;
    }
    Some(region)
}
fn write_region(path: &PathBuf, region: &Region) {
    let mut out = Vec::with_capacity(6 + 1024 * (2 + Tile::BYTES));
    out.extend_from_slice(b"AMR2");
    let count = region.tiles.iter().filter(|t| t.is_some()).count() as u16;
    out.extend_from_slice(&count.to_le_bytes());
    for (i, t) in region.tiles.iter().enumerate() {
        let Some(t) = t else { continue };
        out.extend_from_slice(&(i as u16).to_le_bytes());
        t.write(&mut out);
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
    /// Stores chunk `key`'s tile; true when it changed.
    fn put(&mut self, key: (i32, i32), tile: Tile) -> bool {
        let region = self.region_mut((key.0 >> 5, key.1 >> 5));
        let idx = ((key.1 & 31) * 32 + (key.0 & 31)) as usize;
        let changed = region.tiles[idx].as_ref().is_none_or(|t| {
            t.rgb != tile.rgb || t.height != tile.height || t.depth != tile.depth
        });
        if changed {
            region.tiles[idx] = Some(Box::new(tile));
            region.dirty = true;
            self.revision = self.revision.wrapping_add(1);
        }
        changed
    }
    /// A block column: (colour, height, depth), or none when unknown.
    #[inline]
    fn column(&self, x: i32, z: i32) -> Option<([u8; 3], i16, u8)> {
        let t = self.tile(x >> 4, z >> 4)?;
        let i = ((z & 15) * 16 + (x & 15)) as usize;
        let rgb = [t.rgb[i * 3], t.rgb[i * 3 + 1], t.rgb[i * 3 + 2]];
        (rgb != [0, 0, 0]).then_some((rgb, t.height[i], t.depth[i]))
    }
    /// A column as the map shows it: lit from the north-west by the slope
    /// to its neighbours, walls darkened. 0 for unknown.
    fn shaded(&self, x: i32, z: i32) -> Option<[f32; 3]> {
        let (rgb, h, depth) = self.column(x, z)?;
        let f = if depth == WALL_DEPTH {
            0.5
        } else if depth > 0 {
            1.0
        } else {
            let hn = self.column(x, z - 1).map_or(h, |c| c.1);
            let hw = self.column(x - 1, z).map_or(h, |c| c.1);
            let diff = f32::from(h - hn) + f32::from(h - hw);
            1.0 + (diff * 0.07).clamp(-0.32, 0.22)
        };
        Some([f32::from(rgb[0]) * f, f32::from(rgb[1]) * f, f32::from(rgb[2]) * f])
    }
}

/// The directory of a place's map.
fn place_dir(game: &Game<'_>, place: &Place) -> PathBuf {
    let mut dir = game
        .native_data_dir(WORLDMAP)
        .join("worlds")
        .join(&place.world)
        .join(sanitize(&place.dim));
    if let Some(sub) = place.layer.folder() {
        dir = dir.join(sub);
    }
    dir
}

/// A new world, dimension or layer, or none: save the old, start the new.
fn switch_place(s: &mut State, game: &Game<'_>, now: Option<Place>) {
    if s.place == now {
        return;
    }
    flush(&mut s.explorer);
    s.explorer = Explorer::default();
    s.cache = MapCache::default();
    let same_world = matches!((&s.place, &now), (Some(a), Some(b)) if a.world == b.world);
    if !same_world {
        s.palette = None;
        s.sync = SyncState::default();
    }
    if let Some(place) = &now {
        s.explorer.dir = Some(place_dir(game, place));
        if !same_world {
            s.waypoints = load_waypoints(game, &place.world);
        }
    } else {
        s.waypoints.clear();
        s.screen = None;
        s.dialog = None;
        s.pending = None;
    }
    s.place = now;
}

/// The level the player is in: on a server its own key (`minecraft:lobby`,
/// a server's own End…), which Horizon's `Dim` folds into the three vanilla
/// dimensions; in single player the dimension.
fn level(game: &Game<'_>) -> String {
    match game.remote.as_ref() {
        Some(r) if !r.level.is_empty() => r.level.clone(),
        _ => game.dim.id().to_owned(),
    }
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
fn rgb_of(c: u32) -> [f32; 3] {
    [(c >> 16 & 255) as f32, (c >> 8 & 255) as f32, (c & 255) as f32]
}
/// A block's colour on the map: its map colour, or the biome's grass,
/// foliage or water colour for the blocks the game tints.
fn block_rgb(p: &Palette, biomes: &Biomes, state: u32, biome: u16, x: i32, z: i32) -> Option<[f32; 3]> {
    let (id, _) = p.get(state);
    let scale = |c: [f32; 3], k: f32| [c[0] * k, c[1] * k, c[2] * k];
    match p.tint(state) {
        TINT_GRASS => Some(scale(rgb_of(biomes.grass(biome, f64::from(x), f64::from(z))), 0.92)),
        TINT_FOLIAGE => Some(scale(rgb_of(biomes.get(biome).foliage), 0.78)),
        TINT_SPRUCE => Some(scale(rgb_of(0x61_9961), 0.78)),
        TINT_BIRCH => Some(scale(rgb_of(0x80_A755), 0.82)),
        TINT_WATER => Some(rgb_of(biomes.get(biome).water)),
        _ => {
            let id = usize::from(id);
            (id != 0 && id < MAP_COLORS.len()).then(|| rgb_of(MAP_COLORS[id]))
        }
    }
}
/// One column: the first block with a map colour going down. Water shows
/// its floor through it, more faintly the deeper it is. With a cave `cut`,
/// a solid block at that height is a wall and the floor below it is
/// scanned, as Horizon's minimap does (`minimap::scan_column`).
fn scan_column(
    chunk: &Chunk,
    p: &Palette,
    biomes: &Biomes,
    (x, z): (i32, i32),
    top: i32,
    cut: Option<i32>,
) -> Option<([u8; 3], i16, u8)> {
    let (wx, wz) = (chunk.x * 16 + x, chunk.z * 16 + z);
    let mut y = top;
    if let Some(head) = cut {
        let state = chunk.get(x, head, z);
        let (color, flags) = p.get(state);
        if flags & AIR == 0 && color != 0 && flags & FLUID == 0 {
            let rgb = block_rgb(p, biomes, state, chunk.biome(wx, head, wz), wx, wz)?;
            return Some((to_u8(rgb), head as i16, WALL_DEPTH));
        }
        y = top.min(head);
    }
    while y > MIN_Y {
        let state = chunk.get(x, y, z);
        let (color, flags) = p.get(state);
        if color == 0 {
            y -= 1;
            continue;
        }
        let fluid = flags & FLUID;
        if fluid == 0 || flags & STURDY_TOP != 0 {
            let rgb = block_rgb(p, biomes, state, chunk.biome(wx, y, wz), wx, wz)?;
            return Some((to_u8(rgb), y as i16, 0));
        }
        if fluid == LAVA_FLAG {
            return Some((to_u8(rgb_of(MAP_COLORS[usize::from(LAVA)])), y as i16, 0));
        }
        // Water: its colour over the floor's.
        let water = block_rgb(p, biomes, state, chunk.biome(wx, y, wz), wx, wz)
            .unwrap_or_else(|| rgb_of(MAP_COLORS[usize::from(WATER)]));
        let mut below = y - 1;
        let mut depth = 1u8;
        while below > MIN_Y && p.get(chunk.get(x, below, z)).1 & FLUID != 0 {
            depth = depth.saturating_add(1);
            below -= 1;
        }
        let floor_state = chunk.get(x, below, z);
        let floor = block_rgb(p, biomes, floor_state, chunk.biome(wx, below, wz), wx, wz).unwrap_or(water);
        let a = (0.55 + f32::from(depth) * 0.05).min(0.92);
        let mixed = [
            water[0] * a + floor[0] * (1.0 - a),
            water[1] * a + floor[1] * (1.0 - a),
            water[2] * a + floor[2] * (1.0 - a),
        ];
        return Some((to_u8(mixed), y as i16, depth.min(WALL_DEPTH - 1)));
    }
    None
}
fn to_u8(c: [f32; 3]) -> [u8; 3] {
    // Never all zero: that is "unknown".
    let v = |f: f32| f.round().clamp(0.0, 255.0) as u8;
    let out = [v(c[0]), v(c[1]), v(c[2])];
    if out == [0, 0, 0] { [1, 1, 1] } else { out }
}
fn scan_tile(chunk: &Chunk, p: &Palette, biomes: &Biomes, air: u32, cut: Option<i32>) -> Option<Tile> {
    let top = top_of(chunk, air);
    if top < MIN_Y {
        return None;
    }
    let mut tile = Tile::empty();
    let mut any = false;
    for z in 0..16 {
        for x in 0..16 {
            let i = (z * 16 + x) as usize;
            if let Some((rgb, h, d)) = scan_column(chunk, p, biomes, (x, z), top, cut) {
                tile.rgb[i * 3..i * 3 + 3].copy_from_slice(&rgb);
                tile.height[i] = h;
                tile.depth[i] = d;
                any = true;
            }
        }
    }
    any.then_some(tile)
}
fn explore(s: &mut State, game: &Game<'_>, px: f64, pz: f64) {
    let cut = s.cut;
    let share = s.map.share && s.sync.ready;
    let e = &mut s.explorer;
    if e.dir.is_none() {
        return;
    }
    let palette = s.palette.get_or_insert_with(|| Palette::new(game.res.facts()));
    let biomes = game.res.biomes().clone();
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
                let Some(tile) = scan_tile(chunk, palette, &biomes, air, cut) else {
                    continue;
                };
                if e.put(key, tile) && share && !e.outbox.contains(&key) {
                    e.outbox.push(key);
                }
                if started.elapsed() >= SCAN_BUDGET {
                    return;
                }
            }
        }
    }
}

// --- Sharing -----------------------------------------------------------
//
// Messages travel as Horizon's mod messages (`horizon:mod_net`) on the
// channel `ayynoxs_map:sync`, through a relay: a Horizon host runs one for
// its guests (below), a Paper server the ayynoxs map sync plugin. A
// client sends HELLO when it joins and shares only after the relay's
// WELCOME. The first byte is the kind.

pub(crate) const SYNC_CHANNEL: &str = "ayynoxs_map:sync";
const MSG_HELLO: u8 = 1;
const MSG_WELCOME: u8 = 2;
const MSG_WAYPOINT: u8 = 0x10;
const MSG_TILES: u8 = 0x20;
/// Tiles in one message: a message holds 30 000 bytes at most.
const TILES_PER_MESSAGE: usize = 18;
/// Messages a client sends a frame at most.
const SEND_PER_FRAME: usize = 2;

#[derive(Default)]
struct SyncState {
    /// The relay answered: sharing is on for this connection.
    ready: bool,
    hello_at: Option<Instant>,
    hellos: u32,
    /// Messages waiting to leave.
    out: Vec<Vec<u8>>,
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    let n = b.len().min(255);
    out.push(n as u8);
    out.extend_from_slice(&b[..n]);
}
struct Reader<'a>(&'a [u8]);
impl Reader<'_> {
    fn u8(&mut self) -> Option<u8> {
        let (&b, rest) = self.0.split_first()?;
        self.0 = rest;
        Some(b)
    }
    fn i32(&mut self) -> Option<i32> {
        let b = self.0.get(..4)?;
        let v = i32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        self.0 = &self.0[4..];
        Some(v)
    }
    fn str(&mut self) -> Option<String> {
        let n = usize::from(self.u8()?);
        let b = self.0.get(..n)?;
        self.0 = &self.0[n..];
        Some(String::from_utf8_lossy(b).into_owned())
    }
    fn bytes(&mut self, n: usize) -> Option<&[u8]> {
        let b = self.0.get(..n)?;
        self.0 = &self.0[n..];
        Some(b)
    }
}
fn encode_waypoint(w: &Waypoint) -> Vec<u8> {
    let mut out = vec![MSG_WAYPOINT];
    put_str(&mut out, &w.name);
    for v in [w.x, w.y, w.z] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    put_str(&mut out, &w.dim);
    out.push(w.color as u8);
    out
}

/// Sends one message to wherever this game's relay is: the server, or
/// this process's own relay when it hosts.
fn send_sync(game: &Game<'_>, data: &[u8]) {
    if game.remote.is_some() {
        let message = horizon_protocol::mod_net::Message {
            channel: SYNC_CHANNEL.into(),
            data: data.to_vec(),
        };
        if let Ok(payload) = horizon_protocol::mod_net::encode(&message) {
            game.send(horizon_protocol::packets::play::Serverbound::CustomPayload(
                horizon_protocol::packets::configuration::CustomPayload {
                    channel: horizon_protocol::mod_net::CHANNEL.into(),
                    data: payload,
                },
            ));
        }
    } else if let Ok(mut r) = RELAY.lock() {
        r.from_host.push(data.to_vec());
    }
}

/// Once a frame: say hello, send what waits, read what the host's relay
/// passed on.
fn sync_frame(s: &mut State, game: &mut Game<'_>) {
    let hosting = game.remote.is_none() && game.lan.is_some();
    let joined = game.remote.as_ref().is_some_and(|r| r.joined);
    if hosting {
        s.sync.ready = true;
        if let Ok(mut r) = RELAY.lock() {
            r.window_seen = Some(Instant::now());
            let inbox = std::mem::take(&mut r.to_host);
            drop(r);
            for m in inbox {
                receive(s, game, &m);
            }
        }
    } else if joined && !s.sync.ready && s.sync.hellos < 3 {
        let due = s.sync.hello_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(10));
        if due {
            // Bukkit sends plugin messages only on channels a client registered.
            game.send(horizon_protocol::packets::play::Serverbound::CustomPayload(
                horizon_protocol::packets::configuration::CustomPayload {
                    channel: "minecraft:register".into(),
                    data: horizon_protocol::mod_net::CHANNEL.as_bytes().to_vec(),
                },
            ));
            send_sync(game, &[MSG_HELLO]);
            s.sync.hello_at = Some(Instant::now());
            s.sync.hellos += 1;
        }
    }
    if !s.sync.ready || !s.map.share {
        s.sync.out.clear();
        s.explorer.outbox.clear();
        return;
    }
    // Explored tiles, a few messages a frame.
    let mut budget = SEND_PER_FRAME;
    while budget > 0 && !s.sync.out.is_empty() {
        let m = s.sync.out.remove(0);
        send_sync(game, &m);
        budget -= 1;
    }
    let Some(place) = s.place.clone() else {
        return;
    };
    while budget > 0 && !s.explorer.outbox.is_empty() {
        let n = s.explorer.outbox.len().min(TILES_PER_MESSAGE);
        let keys: Vec<(i32, i32)> = s.explorer.outbox.drain(..n).collect();
        let mut out = vec![MSG_TILES];
        put_str(&mut out, &place.dim);
        out.extend_from_slice(&place.layer.wire().to_le_bytes());
        let tiles: Vec<((i32, i32), &Tile)> = keys
            .iter()
            .filter_map(|&k| s.explorer.tile(k.0, k.1).map(|t| (k, t)))
            .collect();
        out.push(tiles.len() as u8);
        for ((cx, cz), t) in tiles {
            out.extend_from_slice(&cx.to_le_bytes());
            out.extend_from_slice(&cz.to_le_bytes());
            t.write(&mut out);
        }
        send_sync(game, &out);
        budget -= 1;
    }
}

/// A message from the relay.
fn receive(s: &mut State, game: &mut Game<'_>, data: &[u8]) {
    let mut r = Reader(data);
    match r.u8() {
        Some(MSG_WELCOME) => s.sync.ready = true,
        Some(MSG_WAYPOINT) => {
            let (Some(name), Some(x), Some(y), Some(z), Some(dim), Some(color)) =
                (r.str(), r.i32(), r.i32(), r.i32(), r.str(), r.u8())
            else {
                return;
            };
            let name = clean_name(&name);
            let spot = (f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5);
            if name.is_empty() || nearest(s, &dim, spot, 1.5).is_some() {
                return;
            }
            let pos = format!("{x} {y} {z}");
            chat(s, game, &fill(tr(s.lang, T::MsgReceived), &[&name, &pos]));
            s.waypoints.push(Waypoint {
                name,
                x,
                y,
                z,
                dim,
                color: usize::from(color) % COLORS.len(),
                death: false,
            });
            save_waypoints(s, game);
        }
        Some(MSG_TILES) => {
            let (Some(dim), Some(layer), Some(count)) = (r.str(), r.i32(), r.u8()) else {
                return;
            };
            let layer = Layer::from_wire(layer);
            let mut tiles = Vec::new();
            for _ in 0..count {
                let (Some(cx), Some(cz)) = (r.i32(), r.i32()) else {
                    return;
                };
                let Some(t) = r.bytes(Tile::BYTES).and_then(Tile::read) else {
                    return;
                };
                tiles.push(((cx, cz), t));
            }
            merge_tiles(s, game, &dim, layer, tiles);
        }
        _ => {}
    }
}

/// A waypoint a player sent in a private message (`/wp share <nr>
/// <player>`): `x y z colour level name`. The sender's own echo of it finds
/// the waypoint already there and adds nothing.
fn chat_waypoint(s: &mut State, game: &mut Game<'_>, line: &str) {
    if !s.map.share {
        return;
    }
    let mut parts = line.split_whitespace();
    let (Some(x), Some(y), Some(z), Some(color), Some(dim)) = (
        parts.next().and_then(|v| v.parse::<i32>().ok()),
        parts.next().and_then(|v| v.parse::<i32>().ok()),
        parts.next().and_then(|v| v.parse::<i32>().ok()),
        parts.next().and_then(|v| v.parse::<usize>().ok()),
        parts.next(),
    ) else {
        return;
    };
    let name = clean_name(&parts.collect::<Vec<_>>().join(" "));
    let spot = (f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5);
    if name.is_empty() || nearest(s, dim, spot, 1.5).is_some() {
        return;
    }
    let pos = format!("{x} {y} {z}");
    chat(s, game, &fill(tr(s.lang, T::MsgReceived), &[&name, &pos]));
    s.waypoints.push(Waypoint {
        name,
        x,
        y,
        z,
        dim: dim.to_owned(),
        color: color % COLORS.len(),
        death: false,
    });
    save_waypoints(s, game);
}

/// Another player's tiles: into the open map when they are its place's,
/// else into that place's files.
fn merge_tiles(s: &mut State, game: &Game<'_>, dim: &str, layer: Layer, tiles: Vec<((i32, i32), Tile)>) {
    let Some(place) = s.place.clone() else {
        return;
    };
    if place.dim == dim && place.layer == layer {
        for (key, t) in tiles {
            s.explorer.put(key, t);
        }
        return;
    }
    let other = Place {
        world: place.world,
        dim: dim.to_owned(),
        layer,
    };
    let dir = place_dir(game, &other);
    let _ = fs::create_dir_all(&dir);
    let mut regions: HashMap<(i32, i32), Region> = HashMap::new();
    for ((cx, cz), t) in tiles {
        let rk = (cx >> 5, cz >> 5);
        let region = regions
            .entry(rk)
            .or_insert_with(|| read_region(&region_path(&dir, rk)).unwrap_or_else(Region::new));
        region.tiles[((cz & 31) * 32 + (cx & 31)) as usize] = Some(Box::new(t));
    }
    for (rk, region) in &regions {
        write_region(&region_path(&dir, *rk), region);
    }
}

/// The window's mod message from its server.
pub(super) fn net_from_server(game: &mut Game<'_>, channel: &str, data: &[u8]) {
    if channel != SYNC_CHANNEL || !game.native_window() {
        return;
    }
    with(|s| {
        ensure_settings(s, game);
        receive(s, game, data);
    });
}

/// The relay a Horizon host (single player opened to friends, a LAN world,
/// a dedicated Horizon server) runs: what one player shares goes to the
/// others, and to the host's own window.
#[derive(Default)]
struct Relay {
    players: Vec<u128>,
    /// From guests, for the host's window.
    to_host: Vec<Vec<u8>>,
    /// From the host's window, for the guests.
    from_host: Vec<Vec<u8>>,
    /// The host's window ran lately: a dedicated server has none.
    window_seen: Option<Instant>,
}
static RELAY: Mutex<Relay> = Mutex::new(Relay {
    players: Vec::new(),
    to_host: Vec::new(),
    from_host: Vec::new(),
    window_seen: None,
});

pub(super) fn player_joined(player: u128) {
    if let Ok(mut r) = RELAY.lock() {
        if !r.players.contains(&player) {
            r.players.push(player);
        }
    }
}
pub(super) fn player_left(player: u128) {
    if let Ok(mut r) = RELAY.lock() {
        r.players.retain(|p| *p != player);
    }
}
/// A guest's message: welcome them, or pass it on.
pub(super) fn net_from_client(game: &mut Game<'_>, player: u128, channel: &str, data: &[u8]) {
    if channel != SYNC_CHANNEL || data.is_empty() {
        return;
    }
    if data[0] == MSG_HELLO {
        let _ = game.native_net_send_to_player(player, SYNC_CHANNEL, &[MSG_WELCOME]);
        return;
    }
    let others: Vec<u128> = match RELAY.lock() {
        Ok(mut r) => {
            if r.window_seen.is_some_and(|t| t.elapsed() < Duration::from_secs(5)) && r.to_host.len() < 4096 {
                r.to_host.push(data.to_vec());
            }
            r.players.iter().copied().filter(|p| *p != player).collect()
        }
        Err(_) => return,
    };
    for p in others {
        let _ = game.native_net_send_to_player(p, SYNC_CHANNEL, data);
    }
}
/// Each server tick: what the host's window shared goes to every guest.
pub(super) fn relay_tick(game: &mut Game<'_>) {
    if game.native_window() {
        return;
    }
    let (messages, players) = match RELAY.lock() {
        Ok(mut r) => (std::mem::take(&mut r.from_host), r.players.clone()),
        Err(_) => return,
    };
    for m in messages.into_iter().take(32) {
        for &p in &players {
            let _ = game.native_net_send_to_player(p, SYNC_CHANNEL, &m);
        }
    }
}

// --- Hooks: frame, keys, commands --------------------------------------

/// Once a frame: follow the world and the layer, note a death, explore,
/// share, save.
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
        let nether = game.remote.as_ref().map_or(p.dimension == "minecraft:the_nether", |r| r.dimension.has_ceiling);
        let world = world_key(s, game);
        // A screen of ours closed by something else (death, a kick).
        if (s.screen.is_some() || s.dialog.is_some()) && game.screen != Screen::Advancements {
            s.screen = None;
            s.dialog = None;
        }
        // The name window: open it, or end it as its keys asked.
        if s.open_dialog {
            s.open_dialog = false;
            if s.dialog.is_some() && (game.native_playing() || s.screen.is_some()) {
                game.screen = Screen::Advancements;
                game.capture(false);
            } else {
                s.dialog = None;
            }
        }
        if let Some(end) = s.dialog_end.take() {
            end_dialog(s, game, end);
        }
        // The layer follows the player once it has settled; never while
        // the map is open (the game is paused, the view must stay).
        let cut = cut_now(game, p.x, p.y, p.z, nether);
        let layer = Layer::of(cut);
        let moved = s
            .place
            .as_ref()
            .is_none_or(|pl| pl.world != world || pl.dim != level(game));
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
                dim: level(game),
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
                dim: level(game),
                color: 0,
                death: true,
            });
            save_waypoints(s, game);
        }
        s.dead = dead;
        let in_layer = s.place.as_ref().is_some_and(|pl| pl.layer == Layer::of(s.cut));
        if game.native_on(WORLDMAP) && s.map.explore && in_layer {
            explore(s, game, p.x, p.z);
        }
        if game.native_on(WORLDMAP) {
            sync_frame(s, game);
        }
        for line in horizon_client_render::ayynoxs_chat::take() {
            chat_waypoint(s, game, &line);
        }
        if s.last_save.elapsed() >= SAVE_EVERY {
            flush(&mut s.explorer);
            s.last_save = Instant::now();
        }
    });
}

/// The address of the server being joined, for its own map folder.
pub(super) fn connecting(address: &str) {
    with(|s| {
        s.server = address.to_owned();
        s.sync = SyncState::default();
    });
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
    s.dialog = None;
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

/// Asks for a waypoint's name at `at`: the existing one there is renamed.
fn ask_name(s: &mut State, at: (i32, i32, i32), dim: &str) {
    let spot = (f64::from(at.0) + 0.5, f64::from(at.1), f64::from(at.2) + 0.5);
    let rename = nearest(s, dim, spot, 2.0).filter(|&i| !s.waypoints[i].death);
    let text = rename.map_or(String::new(), |i| s.waypoints[i].name.clone());
    s.dialog = Some(NameDialog {
        text,
        at,
        dim: dim.to_owned(),
        rename,
    });
    s.open_dialog = true;
}
fn end_dialog(s: &mut State, game: &mut Game<'_>, end: DialogEnd) {
    let Some(d) = s.dialog.take() else {
        return;
    };
    if end == DialogEnd::Save {
        match d.rename {
            Some(i) => rename_waypoint(s, game, i, &d.text),
            None => add_waypoint(s, game, &d.text, d.at, &d.dim),
        }
    }
    // Back to the map when it is under the window, else to the world.
    if s.screen.is_none() {
        game.screen = Screen::Playing;
        game.capture(true);
    }
}

/// The waypoint the crosshair aims at in the world, else the one the
/// player stands on.
fn aimed(s: &State, game: &Game<'_>, p: &NativePlayer) -> Option<usize> {
    let (w, h) = game.ui_size();
    let best = project_waypoints(s, game, p, (w, h))
        .into_iter()
        .map(|(i, x, y, _)| (i, (x - w / 2.0).hypot(y - h / 2.0)))
        .filter(|&(_, d)| d <= AIM)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i);
    best.or_else(|| nearest(s, &level(game), (p.x, p.y, p.z), 2.0))
}

/// A key went down or up, on any screen.
pub(super) fn key(game: &mut Game<'_>, key: &str, pressed: bool) {
    with(|s| {
        ensure_settings(s, game);
        // The name window takes its keys through the screen hooks.
        if s.dialog.is_some() {
            return;
        }
        let open_key = game.native_mod_key(WORLDMAP, KEY_OPEN, "key.keyboard.m");
        let wp_key = game.native_mod_key(MINIMAP, KEY_WAYPOINT, "key.keyboard.b");
        let rm_key = game.native_mod_key(MINIMAP, KEY_REMOVE, "key.keyboard.backspace");
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
                let h = s.explorer.column(bx, bz).map_or(64, |c| i32::from(c.1) + 1);
                let dim = s.place.as_ref().map(|pl| pl.dim.clone()).unwrap_or_default();
                ask_name(s, (bx, h, bz), &dim);
            } else if key == rm_key || matches!(key, "key.keyboard.delete" | "key.keyboard.backspace") {
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
        let Some(p) = game.native_player() else {
            return;
        };
        if key == wp_key {
            let at = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
            ask_name(s, at, &level(game));
        } else if key == rm_key {
            match aimed(s, game, &p) {
                Some(i) => remove_waypoint(s, game, i),
                None => chat(s, game, tr(s.lang, T::MsgNotFound)),
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

/// A character typed on a screen: the name window's text.
pub(super) fn dialog_char(c: char, canceled: &mut bool) {
    with(|s| {
        if let Some(d) = s.dialog.as_mut() {
            *canceled = true;
            // The waypoint key's own character, before the window is up.
            if s.open_dialog {
                return;
            }
            if !c.is_control() && d.text.chars().count() < NAME_MAX {
                d.text.push(c);
            }
        }
    });
}
/// A key on a screen: the name window's Enter, Escape and Backspace; it
/// keeps every other key from the screen under it.
pub(super) fn dialog_key(key: &str, canceled: &mut bool) {
    with(|s| {
        let Some(d) = s.dialog.as_mut() else {
            return;
        };
        *canceled = true;
        match key {
            "key.keyboard.enter" | "key.keyboard.keypad.enter" => s.dialog_end = Some(DialogEnd::Save),
            "key.keyboard.escape" => s.dialog_end = Some(DialogEnd::Cancel),
            "key.keyboard.backspace" => {
                d.text.pop();
            }
            _ => {}
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
        let index = |n: &str, len: usize| n.parse::<usize>().ok().filter(|&i| i >= 1 && i <= len).map(|i| i - 1);
        match sub {
            "add" | "set" => {
                let at = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
                let dim = level(game);
                add_waypoint(s, game, &rest, at, &dim);
            }
            "del" | "delete" | "remove" | "rm" => {
                let n = rest.trim();
                if n.is_empty() {
                    match nearest(s, &level(game), (p.x, p.y, p.z), f64::INFINITY) {
                        Some(i) => remove_waypoint(s, game, i),
                        None => chat(s, game, tr(l, T::MsgNotFound)),
                    }
                    return;
                }
                let before = s.waypoints.len();
                if let Some(i) = index(n, s.waypoints.len()) {
                    s.waypoints.remove(i);
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
                let i = parts.next().and_then(|n| index(n, s.waypoints.len()));
                let name = clean_name(parts.next().unwrap_or(""));
                match i {
                    Some(i) if !name.is_empty() => rename_waypoint(s, game, i, &name),
                    _ => chat(s, game, tr(l, T::MsgHelp)),
                }
            }
            "share" => {
                // `/wp share <nr|name> [player]`: to one player as a private
                // message (any server), else to everyone through the relay.
                let mut parts = rest.split_whitespace();
                let n = parts.next().unwrap_or("");
                let to = parts.next();
                let i = index(n, s.waypoints.len())
                    .or_else(|| s.waypoints.iter().position(|w| w.name.eq_ignore_ascii_case(n)));
                let Some(i) = i else {
                    chat(s, game, tr(l, T::MsgNotFound));
                    return;
                };
                let w = s.waypoints[i].clone();
                match to {
                    Some(player) if game.remote.is_some() => {
                        let marker = horizon_client_render::ayynoxs_chat::MARKER;
                        let line = format!(
                            "msg {player} {marker} {} {} {} {} {} {}",
                            w.x, w.y, w.z, w.color, w.dim, w.name
                        );
                        game.send_command(&line);
                        chat(s, game, &fill(tr(l, T::MsgSentTo), &[&w.name, player]));
                    }
                    _ if s.sync.ready => {
                        s.sync.out.push(encode_waypoint(&w));
                        chat(s, game, &fill(tr(l, T::MsgShared), &[&w.name]));
                    }
                    _ => chat(s, game, tr(l, T::MsgNoSync)),
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
                        let here = if w.dim == level(game) {
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

/// The waypoints of the player's dimension on screen (GUI pixels), with
/// their distance, projected from the last frame's camera as the locator
/// bar does (`game/locator_bar.rs`).
fn project_waypoints(s: &State, game: &Game<'_>, p: &NativePlayer, (w, h): (f32, f32)) -> Vec<(usize, f32, f32, f64)> {
    let Some((cam, yaw, pitch, fov)) = game.locator_camera else {
        return Vec::new();
    };
    let (sy, cy) = f64::from(yaw.to_radians()).sin_cos();
    let (sp, cp) = f64::from(pitch.to_radians()).sin_cos();
    let forward = [-sy * cp, -sp, cy * cp];
    let up = [-sy * sp, cp, cy * sp];
    let right = [-cy, 0.0, -sy];
    let focal = 1.0 / f64::from((fov / 2.0).to_radians()).tan();
    let aspect = f64::from(w / h.max(1.0));
    let mut out = Vec::new();
    for (i, wp) in s.waypoints.iter().enumerate().filter(|(_, wp)| wp.dim == level(game)) {
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
        let dist = ((target[0] - p.x).powi(2)
            + (f64::from(wp.y) - p.y).powi(2)
            + (target[2] - p.z).powi(2))
        .sqrt();
        out.push((i, ((nx + 1.0) / 2.0) as f32 * w, ((1.0 - ny) / 2.0) as f32 * h, dist));
    }
    out
}

/// The waypoints in the world: a marker and `name · 123 m` over each one
/// on screen; the one the crosshair aims at stands out.
fn world_markers(s: &State, game: &Game<'_>, batch: &mut HudBatch, p: &NativePlayer) {
    let (w, h) = (batch.width, batch.height);
    let atlas = &game.gui.atlas;
    let points = project_waypoints(s, game, p, (w, h));
    let aimed = points
        .iter()
        .map(|&(i, x, y, _)| (i, (x - w / 2.0).hypot(y - h / 2.0)))
        .filter(|&(_, d)| d <= AIM)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i);
    for (i, x, y, dist) in points {
        let wp = &s.waypoints[i];
        let color = if wp.death { 0xFFAAAAAA } else { COLORS[wp.color] };
        let focus = aimed == Some(i);
        marker(batch, game, x, y, if focus { 4.0 } else { 3.0 }, color);
        let label = format!("{} · {} m", wp.name, dist.round() as i64);
        let tw = HudBatch::text_width(atlas, &label);
        let (lx, ly) = ((x - tw / 2.0).round(), (y - 16.0).round());
        let bg = if focus { 0xA0000000 } else { 0x60000000 };
        batch.fill(atlas, lx - 2.0, ly - 1.0, tw + 3.0, 10.0, argb(bg));
        batch.text(atlas, &label, lx, ly, argb(color), true);
    }
}

/// The HUD after Horizon's minimap: the waypoints in the world, the info
/// box under the minimap, and the waypoints on it.
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
        for w in s.waypoints.iter().filter(|w| w.dim == level(game)) {
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
fn dim_name(lang: usize, dim: &str) -> String {
    match dim {
        "minecraft:overworld" => tr(lang, T::Overworld).into(),
        "minecraft:the_nether" => tr(lang, T::Nether).into(),
        "minecraft:the_end" => tr(lang, T::End).into(),
        other => other.into(),
    }
}

/// The map's cells for a view: each the average of up to 3 x 3 blocks
/// under it, shaded, as HUD colours (0 unknown). Recomputed only when the
/// view or a tile changed.
fn map_cells(e: &Explorer, cache: &mut MapCache, view: (f64, f64, f32), cols: i32, rows: i32, step: f64) -> Vec<u32> {
    let (left, top, zoom) = view;
    let key = (left.to_bits(), top.to_bits(), zoom.to_bits(), cols as u32, rows as u32, e.revision);
    if cache.key == Some(key) {
        return cache.cells.clone();
    }
    let k = (step.round() as i32).clamp(1, 3);
    let mut cells = Vec::with_capacity((cols * rows) as usize);
    for j in 0..rows {
        for i in 0..cols {
            let (x0, z0) = (left + f64::from(i) * step, top + f64::from(j) * step);
            let mut sum = [0.0f32; 3];
            let mut n = 0.0f32;
            for sz in 0..k {
                for sx in 0..k {
                    let x = (x0 + (f64::from(sx) + 0.5) * step / f64::from(k)).floor() as i32;
                    let z = (z0 + (f64::from(sz) + 0.5) * step / f64::from(k)).floor() as i32;
                    if let Some(c) = e.shaded(x, z) {
                        sum[0] += c[0];
                        sum[1] += c[1];
                        sum[2] += c[2];
                        n += 1.0;
                    }
                }
            }
            cells.push(if n == 0.0 {
                0
            } else {
                // A few low bits off, so flat ground draws as long runs.
                let q = |v: f32| ((v / n).round().clamp(0.0, 255.0) as u8) & 0xFC;
                rgba(q(sum[0]), q(sum[1]), q(sum[2]), 255)
            });
        }
    }
    cache.key = Some(key);
    cache.cells = cells.clone();
    cells
}

/// Draws the map instead of the Advancements screen while it is open, and
/// the name window over it (or over the world).
pub(super) fn draw(game: &mut Game<'_>, batch: &mut HudBatch) -> std::ops::ControlFlow<()> {
    with(|s| {
        if s.screen.is_none() && s.dialog.is_none() {
            return std::ops::ControlFlow::Continue(());
        }
        if s.screen.is_some() {
            draw_map(s, game, batch);
        } else {
            let atlas = &game.gui.atlas;
            batch.fill(atlas, 0.0, 0.0, batch.width, batch.height, argb(0x60000000));
        }
        if let Some(d) = &s.dialog {
            draw_dialog(s.lang, d, game, batch);
        }
        std::ops::ControlFlow::Break(())
    })
}

fn draw_dialog(l: usize, d: &NameDialog, game: &Game<'_>, batch: &mut HudBatch) {
    let atlas = &game.gui.atlas;
    let (w, h) = (batch.width, batch.height);
    let (bw, bh) = (220.0, 64.0);
    let (x, y) = (((w - bw) / 2.0).round(), ((h - bh) / 2.0).round());
    batch.fill(atlas, x - 1.0, y - 1.0, bw + 2.0, bh + 2.0, argb(0xFF5A5A5A));
    batch.fill(atlas, x, y, bw, bh, argb(0xF0101418));
    let title = tr(l, if d.rename.is_some() { T::RenameTitle } else { T::NameTitle });
    let tw = HudBatch::text_width(atlas, title);
    batch.text(atlas, title, (x + (bw - tw) / 2.0).round(), y + 6.0, argb(0xFFFFFF55), true);
    // The text box, with a blinking caret.
    let (fx, fy, fw, fh) = (x + 10.0, y + 20.0, bw - 20.0, 16.0);
    batch.fill(atlas, fx - 1.0, fy - 1.0, fw + 2.0, fh + 2.0, argb(0xFFA0A0A0));
    batch.fill(atlas, fx, fy, fw, fh, argb(0xFF000000));
    let caret = if game.started.elapsed().as_millis() / 500 % 2 == 0 { "_" } else { "" };
    let shown = format!("{}{caret}", d.text);
    batch.text(atlas, &shown, fx + 4.0, fy + 4.0, u32::MAX, true);
    let pos = format!("{} {} {}", d.at.0, d.at.1, d.at.2);
    batch.text(atlas, &pos, fx, y + 41.0, argb(0xFF909090), false);
    let hint = tr(l, T::NameHint);
    let hw = HudBatch::text_width(atlas, hint);
    batch.text(atlas, hint, x + bw - 10.0 - hw, y + 52.0, argb(0xFFAAAAAA), true);
}

fn draw_map(s: &mut State, game: &mut Game<'_>, batch: &mut HudBatch) {
    let Some(m) = s.screen.as_mut() else {
        return;
    };
    let l = s.lang;
    let (w, h) = (batch.width, batch.height);
    // Keys held pan the map.
    let dt = m.last.elapsed().as_secs_f32().min(0.1);
    m.last = Instant::now();
    if s.dialog.is_none() {
        let speed = f64::from(400.0 * dt / m.zoom);
        m.cz += speed * (f64::from(u8::from(m.held[1])) - f64::from(u8::from(m.held[0])));
        m.cx += speed * (f64::from(u8::from(m.held[3])) - f64::from(u8::from(m.held[2])));
    }
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
    let cell = zoom.max(w / MAX_COLUMNS);
    let step = f64::from(cell / zoom);
    // Cells line up with blocks when a block covers several pixels.
    let aligned = cell <= zoom;
    let (base_x, base_z) = if aligned { (left.floor(), top.floor()) } else { (left, top) };
    let (ox, oz) = if aligned {
        (((base_x - left) * f64::from(zoom)) as f32, ((base_z - top) * f64::from(zoom)) as f32)
    } else {
        (0.0, 0.0)
    };
    let cols = (w / cell).ceil() as i32 + 1;
    let rows = (h / cell).ceil() as i32 + 1;
    let cells = map_cells(e, &mut s.cache, (base_x, base_z, zoom), cols, rows, step);
    for j in 0..rows {
        let y = oz + j as f32 * cell;
        let row = &cells[(j * cols) as usize..((j + 1) * cols) as usize];
        let mut start = 0usize;
        for i in 1..=row.len() {
            if i < row.len() && row[i] == row[start] {
                continue;
            }
            if row[start] != 0 {
                let x = ox + start as f32 * cell;
                batch.fill(atlas, x, y, (i - start) as f32 * cell, cell, row[start]);
            }
            start = i;
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
    let synced = if s.sync.ready && s.map.share {
        format!(" – {}", tr(l, T::Synced))
    } else {
        String::new()
    };
    let title = format!("{} – {} – {}{synced}", tr(l, T::WorldTitle), dim_name(l, &dim), layer);
    let tw = HudBatch::text_width(atlas, &title);
    batch.text(atlas, &title, ((w - tw) / 2.0).round(), 3.0, u32::MAX, true);
    batch.fill(atlas, 0.0, h - 26.0, w, 26.0, argb(0xA0000000));
    let at = match e.column(hovered.0, hovered.1) {
        Some((_, height, _)) => format!("X {}  Y {}  Z {}", hovered.0, height, hovered.1),
        None => format!("X {}  Z {}", hovered.0, hovered.1),
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
}

/// The left button on the map: drag it.
pub(super) fn mouse(game: &mut Game<'_>, pressed: bool) -> std::ops::ControlFlow<()> {
    with(|s| {
        if s.dialog.is_some() && s.screen.is_none() {
            return std::ops::ControlFlow::Break(());
        }
        let Some(m) = s.screen.as_mut() else {
            return std::ops::ControlFlow::Continue(());
        };
        m.drag = (pressed && s.dialog.is_none()).then_some(game.cursor);
        std::ops::ControlFlow::Break(())
    })
}
pub(super) fn drag(game: &mut Game<'_>) -> std::ops::ControlFlow<()> {
    with(|s| {
        if s.dialog.is_some() && s.screen.is_none() {
            return std::ops::ControlFlow::Break(());
        }
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
        if s.dialog.is_some() && s.screen.is_none() {
            return std::ops::ControlFlow::Break(());
        }
        let Some(m) = s.screen.as_mut() else {
            return std::ops::ControlFlow::Continue(());
        };
        if y != 0.0 && s.dialog.is_none() {
            zoom_map(m, y.signum(), cursor, size);
        }
        std::ops::ControlFlow::Break(())
    })
}
/// Esc (or the Advancements key) closes the name window first, then the
/// map.
pub(super) fn close(game: &mut Game<'_>) -> std::ops::ControlFlow<()> {
    with(|s| {
        if s.dialog.is_some() {
            end_dialog(s, game, DialogEnd::Cancel);
            return std::ops::ControlFlow::Break(());
        }
        if s.screen.is_none() {
            return std::ops::ControlFlow::Continue(());
        }
        close_map(s, game);
        std::ops::ControlFlow::Break(())
    })
}
