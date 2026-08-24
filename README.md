# Tree Archiver

A Windows desktop app for planning archives of large directory trees, then
building them — as `.tar`, `.tar.gz` or `.7z`. Rust + Tauri 2 behind a React
tree view.

Existing tools make you choose between writing exclusion patterns blind and
ticking thousands of individual files. This one lets you see each branch's size,
decide what goes in, save that decision as a reusable plan, and execute it.

## Running it

```
npm install
npm run tauri dev      # development, with hot reload
npm run tauri build    # NSIS + MSI installers in src-tauri/target/release/bundle
```

Requires Rust (MSVC toolchain), Node 20+, and the WebView2 runtime, which ships
with Windows 10/11.

Prebuilt installers are attached to each [release](https://github.com/dlvoy/tree-archiver/releases).
See [BUILDING.md](BUILDING.md) for the full toolchain setup, the release
pipeline, and how to cut a version.

## Designing an archive

Drop folders anywhere in the window, or use **Add folders** / **Add files**.
Everything you add arrives collapsed and fully checked — uncheck what you want
to leave out.

- The tree root is always the topmost folder common to everything you have
  added. Directories between that root and your sources appear as
  **pass-through** nodes and hold only the branches you added, never their
  unlisted siblings on disk.
- Each directory's loose files are grouped under a `<files>` pseudo-folder, so a
  folder with 4,000 files is one row until you open it. Unchecking the group
  drops every file in it while leaving subfolders alone.
- The size column shows `selected / total` for partly-selected rows, with a
  hairline beneath filled to that proportion. A full rule means the whole branch
  is going in; an empty track means none of it.
- Sort by name or size, ascending or descending. Size sorting uses each branch's
  size on disk, so rows do not move around while you are ticking boxes.

Keyboard: arrows move and expand/collapse, <kbd>Space</kbd> toggles a row,
<kbd>Ctrl</kbd>+<kbd>A</kbd> checks everything, <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>A</kbd> clears it.

## The plan file

**Save** writes a JSON plan. The baseline is "everything under `sources`", and
`rules` subtract from it — so unchecking a folder is recorded as a single rule
covering that whole branch, never as a list of the files inside it:

```json
{
  "version": 1,
  "root": "C:\\Users\\Nick",
  "sources": ["C:\\Users\\Nick\\.android", "C:\\Users\\Nick\\.atom"],
  "sort": { "by": "size", "dir": "desc" },
  "output": { "compression": "none", "gzipLevel": 6, "sevenzLevel": 6, "sevenzSolid": false },
  "rules": [
    { "path": ".android/avd", "scope": "tree", "action": "exclude" },
    { "path": ".atom/packages", "scope": "files", "action": "exclude" },
    { "path": ".atom/config.cson", "scope": "file", "action": "exclude" }
  ]
}
```

| Scope | Covers |
|---|---|
| `tree` | the folder and everything beneath it |
| `files` | only a folder's direct files — its `<files>` group; subfolders survive |
| `file` | one file |

Paths are relative to `root` with forward slashes. Rules apply in order and the
last one wins, so a hand-written `include` rule can carve an exception out of an
excluded branch. **Open** rescans the sources and reapplies the rules; anything
that no longer exists is reported rather than silently dropped.

## Building the archive

**Archive…** asks for an output path and shows the spec. For an uncompressed
`.tar` the predicted size is exact — it is computed from the real tar block
layout, not estimated. Choosing gzip or 7z keeps that number as a labelled
upper bound.

Three formats:

| Format | What it writes |
|---|---|
| **None** | a plain `.tar`, the only one whose size is known in advance |
| **gzip** | `.tar.gz`, level 1–9 |
| **7z** | `.7z`, LZMA2 preset 0–9, optionally **solid** |

A **solid** 7z packs every file into one shared stream. It is markedly smaller
on a tree of many small files, but progress is reported more coarsely and
extracting one file means decompressing what came before it. Off by default.

While it runs you get a progress bar, throughput, ETA, and a collapsible log.

- **An unchecked folder contributes nothing** — no directory entry, no contents.
- **A file that cannot be read never stops the run.** It is logged at error
  level and skipped, and the archive completes. Save the log afterwards from the
  same dialog.
- Entries are named relative to the root's parent, so extracting produces one
  top-level folder rather than spraying files into the current directory.
- Cancelling deletes the partial archive.

Directory junctions and symlinks are recorded but never followed, which is what
keeps a scan of a folder containing a link to itself from running forever. Paths
longer than the legacy 260-character limit are handled throughout.

## Layout

```
src/                     React frontend
  api/commands.ts        typed wrappers over the Rust command surface
  store/tree.ts          what has been looked at: nodes, children, expansion
  components/            toolbar, virtualized tree, dialogs
src-tauri/src/
  model/arena.rs         the tree; nodes in a Vec, referenced by index
  model/check.rs         tri-state propagation
  model/sort.rs          natural-order and size comparison
  roots.rs               common-ancestor computation and rebuilds
  scan.rs                threaded directory walking
  plan.rs                plan format, rule compaction and application
  archive.rs             tar and 7z writing, progress, error tolerance
  commands.rs            the IPC surface
```

The Rust side owns the tree. The webview never touches the filesystem — it has
no filesystem capability at all — and never receives the whole tree; it asks for
one node's children at a time and caches them.

## Tests

```
cd src-tauri && cargo test
```

128 tests. The unit tests cover check propagation, re-rooting, natural sort,
plan compaction (including that compaction is a fixpoint over application), the
tar block arithmetic, and both 7z paths — a stream per file and one solid block
— down to an extract-and-compare of the bytes. The integration suite in
`tests/end_to_end.rs` builds a real tree on disk to cover the Windows-specific
hazards: paths past 260 characters, a junction that points at its own ancestor,
a file that disappears between planning and writing, and an extract-and-compare
round trip.

## Licence

MIT — see [LICENSE.txt](LICENSE.txt). The text is compiled into the executable
and shown by **About**, the ⓘ button in the toolbar, alongside the version, the
build date and the commit the build was cut from.
