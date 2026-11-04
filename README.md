# ayynoxs map pack

A native mod for [Horizon](https://playhorizon.dev) (Minecraft 26.3 rewritten in Rust) that adds a **minimap** and a
**full-screen world map**, and lets friends share their waypoints and their explored map. The features take
inspiration from the well-known Java map mods, but every line of code and every graphic here is new.

It adds two entries under *Options > Mods*, and you can turn each one on or off separately:

## ayynoxs minimap
- Built on Horizon's fast minimap renderer: height shading and water, and it switches to a cave or Nether view by itself
- **Coordinates** and **facing direction** below the map
- **Waypoints** drawn as coloured diamonds. A waypoint outside the map's range sits on its edge.
- **Waypoints in the world:** each one shows a marker with its name and its distance in metres. The one your
  crosshair aims at stands out.
- Waypoints belong to the world they were set in: on servers with their own worlds (a lobby, a farm world, a
  server's own End) each world keeps its own.
- A **death point** is set automatically when you die
- Settings: corner, size, zoom, round or square, rotate with the player, coordinates, waypoints, waypoints in the world

| Key (rebindable) | Action |
|---|---|
| `B` | Opens a window for the waypoint's name: type it, press Enter (Esc cancels). Standing on a waypoint, you rename it. |
| `Backspace` | Remove the waypoint your crosshair aims at, or the one you stand on |
| `I` / `O` | Zoom the minimap in / out |

Chat commands:

| Command | Action |
|---|---|
| `/wp add <name>` | Set a waypoint here. On an existing waypoint it renames that one, so they never stack. |
| `/wp del` | Remove the nearest waypoint |
| `/wp del <name or number>` | Remove that waypoint |
| `/wp rename <number> <name>` | Rename a waypoint |
| `/wp share <number> <player>` | Send a waypoint to one player as a private message: works on **every** server |
| `/wp share <number>` | Send a waypoint to everyone (where map sharing is active, see below) |
| `/wp list` | List all waypoints with their number and distance |
| `/wp clear` | Remove every waypoint of this world |
| `/wp help` | Show these commands |

## ayynoxs world map
- **Detailed colours:** grass, leaves and water in their biome's colours, the floor shining through shallow water,
  smooth relief shading lit from the north-west, and cells that average their blocks when zoomed out
- Saves every chunk you have seen, separately for each world, server, dimension and layer
  (`horizon-mods/.horizon-mod-data/ayynoxs_worldmap/`)
- **Layers like the minimap:** under the open sky you see the surface. In a cave, under a roof or in the Nether,
  you see the floor at your head height, with walls cut there. Cave layers are stored in 16-block bands, and the
  title bar shows which layer is open.
- Shows waypoints and your arrow, the coordinates and height under the cursor, and an optional chunk grid

| Key | Action |
|---|---|
| `M` | Open / close the map |
| Drag with the mouse / WASD / arrow keys | Pan |
| Mouse wheel, `I` / `O` | Zoom toward the cursor |
| Space | Jump back to you |
| `B` | Set a waypoint at the cursor (the name window opens) |
| `Backspace` / `Del` | Remove the waypoint under the cursor |
| `Esc` | Close |

## Multiplayer: share the map with friends
**Waypoints, on any server:** `/wp share <number> <player>` sends the waypoint as a private message (`/msg`). If
the other player has the pack, it becomes a waypoint on their map and the message itself stays out of their chat.
This needs nothing on the server. A player without the pack sees a line starting with `[ayynoxs-wp]`.

**The whole map, live:** with *Share map & waypoints in multiplayer* on (the default), players with the pack also
share:
- every chunk they explore, so everyone's world map fills in together, layer by layer
- every new waypoint they set

This part works through Horizon's mod messages, so it needs something in the middle that passes them on:

| Where you play | What you need |
|---|---|
| A Horizon world opened to friends (Play with Friends, LAN), or a dedicated Horizon server | Nothing: the host's game passes everything on |
| A Paper server | The plugin `AyynoxsMapSync-0.4.0.jar` from the [releases](https://github.com/DenoxyYT/ayynoxs-map-pack/releases) in its `plugins` folder |

When sharing is active, the world map's title bar says *shared*. Removing a waypoint stays on your own map, and
chunks explored before you joined are not sent again.

## Languages
The mods start in **English**. You can switch to **Deutsch, Français, Italiano or Русский** with the *Language*
button on either mod's settings page; both mods share the choice. The settings pages, the HUD, the map and chat
messages switch right away. The names in the *Options > Mods* list switch after a restart.

## Install
1. Download `ayynoxs_map_pack-0.4.1.hnmod` from the
   [releases](https://github.com/DenoxyYT/ayynoxs-map-pack/releases).
2. Install it on the **Mods** page of the Horizon launcher.
3. Click **Build** and confirm with "I trust these mods: build".

Native mods run as native code with your user's full permissions, so only build mods whose source you trust.

## Build it yourself
1. Pack the mod:
   ```powershell
   powershell -ExecutionPolicy Bypass -File pack.ps1
   ```
   This writes `dist/ayynoxs_map_pack-0.4.1.hnmod`.
2. Install that file in the launcher as above, or build straight from the source folder:
   ```powershell
   horizon-launcher native-build mixins/ayynoxs_map_pack --trust-native-code
   ```
3. The Paper plugin is in `paper-plugin/` (Java 25, Paper 26.2):
   ```powershell
   cd paper-plugin; gradle build
   ```

## Technical notes
- Pinned to Horizon nightly commit `6475f62` (2026-11-04). Besides the official native hook points, the world map takes
  the slot of the Advancements screen (`advancements_screen`) and hooks `extras_connecting` to read the server
  address. A new Horizon release therefore needs the pin updated and the pack checked again.
- Map files are named `r.X.Z.amr`. Each holds 32×32 chunks, with 3 bytes of colour, 2 bytes of height and 1 byte of
  water depth per block column (`AMR2`). Files from 0.1 to 0.3 (`AMR1`) are still read.
- The scan takes at most 1.5 ms per frame. Chunks near the player are scanned again after blocks change.
- Sharing uses the mod message channel `ayynoxs_map:sync` on `horizon:mod_net`, with at most 18 chunks per message and
  2 messages per frame.

## Ideas for later
- A beam over waypoints in the world
- An entity radar (players and mobs) on the minimap
- Browsing other cave layers on the world map, and a biome display
- Sending a joining player the map the others already explored

## License
MIT, see [LICENSE](LICENSE). Not made or endorsed by Mojang, Microsoft or the Horizon project.
