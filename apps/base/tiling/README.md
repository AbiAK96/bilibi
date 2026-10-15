# bilibi-tiling

The tiled workspace at `/workspace` (Bilimbi `Base.Tiling`): several pages
side by side in one browser tab, arranged by a Hyprland-style dwindle or
master tree, with keyboard control, drag resizing, drag-to-swap and monocle.
`src/layout.rs` is the pure tree and its compact URL form; `src/web.rs` is the
page, which holds only the tree, the focused tile, monocle and the names the
tiles report. This module owns no tables.

## Tiles are pages

A tile is a same-origin `<iframe>` of an existing page at its own URL. The
page runs as its own live session: its own socket, mount, URL state, flash
and access check, exactly as when opened alone, so twenty tiles cost what
twenty tabs cost. Web marks the frame's requests framed (`Sec-Fetch-Dest`),
Base UI renders them chromeless (`templates/framed.html`), and nothing in any
module changes to be shown in a tile. A tile whose page the account may not
open shows the refusal instead of a frame.

- **Frames survive.** Tiles render in creation order, never tree order, and
  are positioned by `data-place` alone, so a split, swap, move, resize, close
  or monocle never moves or re-creates a frame; the live patcher never rewrites
  an existing frame's `src`.
- **One scrollbar.** Inside a tile a page scrolls in the framed `#app-content`
  only, and a list page's table takes the tile's height under a sticky heading
  ("List fill" in `apps/base/ui/assets/css/app.css`). The workspace page itself
  never scrolls.
- **Back leaves the workspace.** A framed page never adds a history entry:
  `apps/base/ui/assets/live.js` replaces its live address changes and turns a
  plain link click into `location.replace`. The Tiling hook reports where each
  frame went (`tile_navigated`), and the workspace records the path in `?t=`
  with a replaced address. The workspace's own changes push, so Back steps
  through them and then leaves; within the workspace Back arrives as a params
  update (`data-history="params"`) and the tree read back is reconciled with
  the one on screen, so frames that stay keep their documents.
- **Narrow screens** (below `lg`) show the focused tile alone with a tab strip
  above to switch; the tree is kept, so widening restores the tiling. The
  sidebar's tile control opens the page normally there (`assets/shell.js`).

## Keys

`apps/base/ui/assets/tiling.js` implements each key and this table is their
documentation; change them together. The listener is attached to every
frame's document too, so the keys work wherever focus is.

| Key | Does |
| --- | --- |
| `Ctrl+.` | Enter or leave tiling mode (shown in the status bar) |
| `h` `j` `k` `l`, arrows | Focus the tile in that direction |
| `H` `J` `K` `L` | Move the focused tile that way |
| `s` | Swap the focused tile with its neighbour in the last direction |
| `r`, then arrows | Resize; `Esc` returns to tiling mode |
| `f` | Fill the workspace with the focused tile, or show every tile again |
| `t` | Flip the split direction, or the master direction in a master layout |
| `m` | Make the focused tile the master, in a master layout |
| `o` | Open the focused tile's page alone |
| `n` | Add a page |
| `q` | Close the focused tile |
| `Esc` | Leave tiling mode (an open menu takes it first) |

Saved layouts (`1` to `9`) and following selections (`w`) join this table
with the saved-layout and follow-channel work.

With a pointer: drag a divider to resize (a focused divider also moves with
its arrow keys), drag a tile's grip onto another tile to swap them, and use
the tile menu for the split flip and close. Pressing anywhere in a tile,
inside its page included, focuses it.

## Try it

Run the app (`./scripts/start-app.sh`, or the manual commands in the root
README), sign in, and use a sidebar row's "Open in a tile" control, or open
`/workspace` and add pages. Then: navigate inside a tile and reload (the tile
keeps its page); press `Ctrl+.` and use the keys above; drag a divider and a
grip; press Back (it undoes the last workspace change, then leaves); narrow the
window below 1024px (one tile and a tab strip).
