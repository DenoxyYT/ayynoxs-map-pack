# ayynoxs map pack

A native mod for [Horizon](https://playhorizon.dev) (Minecraft 26.3 rewritten in Rust) that adds a **minimap** and a
**full-screen world map**. The features take inspiration from the well-known Java map mods, but every line of code
and every graphic here is new.

It adds two entries under *Options > Mods*, and you can turn each one on or off separately:

## ayynoxs minimap
- Built on Horizon's fast minimap renderer: height shading and water, and it switches to a cave or Nether view by itself
- **Coordinates** and **facing direction** below the map
- **Waypoints** drawn as coloured diamonds. A waypoint outside the map's range sits on its edge.
- **Waypoints in the world:** each one shows a marker with its name and its distance in metres, right where it is
- A **death point** is set automatically when you die
- Settings: corner, size, zoom, round or square, rotate with the player, coordinates, waypoints, waypoints in the world

| Key (rebindable) | Action |
|---|---|
| `B` | Opens the chat with `/wp add `: type a name and press Enter. Standing on a waypoint, `B` removes it instead. |
| `I` / `O` | Zoom the minimap in / out |

Chat commands:

| Command | Action |
|---|---|
| `/wp add <name>` | Set a waypoint here. On an existing waypoint it renames that one, so they never stack. |
| `/wp del` | Remove the nearest waypoint |
| `/wp del <name or number>` | Remove that waypoint |
| `/wp rename <number> <name>` | Rename a waypoint |
| `/wp list` | List all waypoints with their number and distance |
| `/wp clear` | Remove every waypoint of this world |
| `/wp help` | Show these commands |

## ayynoxs world map
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
| `B` | Set a waypoint at the cursor |
| `Del` / `Backspace` / `X` | Remove the waypoint under the cursor |
| `Esc` | Close |

## Languages
The mods start in **English**. You can switch to **Deutsch, Français, Italiano or Русский** with the *Language*
button on either mod's settings page; both mods share the choice. The settings pages, the HUD, the map and chat
messages switch right away. The names in the *Options > Mods* list switch after a restart.

## Install
1. Download `ayynoxs_map_pack-0.3.0.hnmod` from the
   [releases](https://github.com/DenoxyYT/ayynoxs-map-pack/releases).
2. Install it on the **Mods** page of the Horizon launcher.
3. Click **Build** and confirm with "I trust these mods: build".

Native mods run as native code with your user's full permissions, so only build mods whose source you trust.

## Build it yourself
1. Pack the mod:
   ```powershell
   powershell -ExecutionPolicy Bypass -File pack.ps1
   ```
   This writes `dist/ayynoxs_map_pack-0.3.0.hnmod`.
2. Install that file in the launcher as above, or build straight from the source folder:
   ```powershell
   horizon-launcher native-build mixins/ayynoxs_map_pack --trust-native-code
   ```

## Technical notes
- Pinned to Horizon nightly commit `2944480` (2026-11-03). Besides the official native hook points, the world map takes the slot of the
  Advancements screen (`advancements_screen`) and hooks `extras_connecting` to read the server address. A new
  Horizon release therefore needs the pin updated and the pack checked again.
- Map files are named `r.X.Z.amr`. Each holds 32×32 chunks, with 1 byte of colour and 2 bytes of height per block column.
- The scan takes at most 1.5 ms per frame. Chunks near the player are scanned again after blocks change.

## Ideas for later
- A beam over waypoints in the world
- An entity radar (players and mobs) on the minimap
- Browsing other cave layers on the world map, and a biome display
- Drawing the world map as a GPU texture instead of rectangles, to stay smooth when zoomed far out

## License
MIT, see [LICENSE](LICENSE). Not made or endorsed by Mojang, Microsoft or the Horizon project.
