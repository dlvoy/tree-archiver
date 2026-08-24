# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.6.0] - 2026-08-24

### Added

- A **Count** sort mode (Toolbar → Sort) orders folders by how many archive
  entries they contribute instead of by size, and the tree's "X / Y" figures
  switch to item counts while it's active.
- A **Save the entry list** button in the Build dialog writes the exact list
  of entries the archive will contain — one line per entry, directories
  slash-terminated, in write order — to a text file without building the
  archive first.

### Fixed

- Starting an archive, or changing the path mode in the Build dialog, no
  longer freezes the window on a large tree: entry collection now runs off
  the main thread, with the dialog's fields disabled and a busy indicator
  shown while it does.

## [1.5.0] - 2026-08-24

### Added

- **AutoIgnore**, a new button in the Selection group that matches a catalog
  of `.gitignore`-style rulesets against the staged tree and unchecks
  whatever they match. Excluded items are tagged **auto** on the row, so
  it stays clear what the tool excluded versus what was unchecked by hand.
  Thirteen built-in presets cover common cases — backups, caches,
  dev-dependency folders, precompiled output, logs, OS and editor metadata,
  crash dumps, and generated build/coverage output among them.
- Custom rulesets can be imported from any file written in `.gitignore`
  syntax, named, and deleted again; built-in presets cannot be deleted.
  Re-checking an auto-excluded item by hand clears its tag for good, even
  across a later Apply of an unrelated ruleset.
- A **Case Insensitive** option in the AutoIgnore dialog, on by default and
  remembered like the ruleset selection, makes patterns like `*cache*` match
  regardless of case.

## [1.4.0] - 2026-08-24

### Added

- An **App interface** setting (Settings → Appearance) chooses how the
  toolbar draws its Sources/Plan/Sort/Selection buttons: icons only
  (the new default), labels only (today's look), or both. The four buttons
  on the right — language, theme, settings, about — are always icon-only.
- A **File archiving order** setting (Settings → Archiving) chooses how files
  are ordered inside the archive, independent of the on-screen tree order:
  **Optimal** (the new default) groups files by compressibility and by name so
  similar and duplicate content lands close together, which gives gzip and 7z
  more redundancy to find; **As in plan** keeps today's tree order; and
  **Alphabetical** sorts by name.

### Changed

- The MIT licence text in the About dialog now opens in a wider dialog, so
  longer lines wrap less.
- The right-click context menu is disabled everywhere in the window. It only
  ever offered browser leftovers like Reload, which would silently discard
  the in-progress tree with no warning. Selecting text and Ctrl+C/Ctrl+V
  copy-paste are unaffected.

## [1.3.0] - 2026-08-24

### Added

- **7z**, a third compression choice beside none and gzip, writing a real
  `.7z` rather than a tar inside one. An LZMA2 level from 0 to 9, and a
  **Solid archive** checkbox that packs every file into one shared stream:
  markedly smaller on a tree of many small files, at the cost of coarser
  progress and slower extraction of any one file. Off by default, because a
  stream per file is what keeps the per-file progress, the ETA and the
  skip-an-unreadable-file rule working the way they do for tar.
- An **About** dialog, opened from the ⓘ button in the toolbar. It carries the
  version, the date the build was made and the commit it was cut from, so a
  report can name the exact build. The version links to that release on GitHub.
- The project now has a licence: MIT, in `LICENSE.txt`. The text is compiled
  into the executable and shown in full from the About dialog, so it travels
  with the application rather than only with the repository.

### Changed

- Building an archive needs Rust 1.93 to compile, up from 1.77, which is what
  the 7z encoder requires. This affects building the app, not running it.
- Preferences and plan files saved with 7z selected cannot be read by 1.2.1 or
  earlier: that version rejects the unknown value, and falls back to default
  preferences or refuses the plan outright. Choosing none or gzip leaves both
  files readable by an older build as before.

### Fixed

- The progress panel could end a run disagreeing with the summary printed just
  above it — four of 61 files beside a message saying 61 were written. The
  final update of a run was being discarded, and it is the one carrying the
  full count.
- A run that finished in under a tenth of a second reported nothing at all and
  left every figure in the panel as a dash. One too brief to measure a rate now
  reports its average instead of a blank.
- A single large file left the panel motionless until it was finished. Progress
  is now reported as the file is read rather than once it is written, so the bar
  moves within a file as well as between files. Most visible with 7z, where one
  file can take tens of seconds.

## [1.2.1] - 2026-08-23

### Fixed

- The Windows uninstall list showed a fragment of the application identifier
  where the publisher should be. It now shows the publisher's full name, in
  both the installer and the MSI package.

## [1.2.0] - 2026-08-23

### Added

- The interface speaks Polish and German as well as English, picked from the
  Windows display language by default. A flag button beside the theme button
  cycles between them, and the choice is remembered.
- A settings dialog, opened from the cog in the toolbar, with named dropdowns
  for theme and language.
- **Integrate with Explorer**, a setting that adds “Archive with Tree
  Archiver” to the right-click menu for files and folders. Choosing it stages
  the selection in the window that is already open rather than starting a
  second copy of the app, and selecting several items produces one staging
  rather than one per item. Both switching it on and switching it off ask for
  confirmation first, and it needs no administrator rights.
- The build log now lists every file and folder as it goes in, and closes with
  the totals: what was written, what could not be read, and how long it took.
  Successful lines read as ordinary text so that failures, in red, are the only
  thing that stands out.

### Changed

- The application identifier is now `pl.dzienia.treearchiver`. Preferences
  saved by an earlier version are carried over automatically the first time
  1.2.0 runs. Because Windows identifies an installed program by that value,
  1.2.0 installs alongside 1.0.0 or 1.1.0 rather than replacing it — uninstall
  the older version by hand if you do not want both.
- The log window keeps the most recent few thousand lines rather than all of
  them, so a very large archive cannot exhaust memory; the complete log is
  still what **Save log** writes. The header says when the view has been
  trimmed.

## [1.1.0] - 2026-08-23

### Added

- The build dialog now chooses how much of each path is kept inside the
  archive: **Folders only**, putting every staged folder at the top;
  **Common root**, keeping the folder they share; or **Full path**, keeping
  everything including the drive letter. Folders only is the default. A layout
  in which two folders would collide and silently merge is offered as
  unavailable, with the clashing name given.
- A third theme setting, **System**, which follows the Windows light or dark
  preference and changes with it while the app is open. It is the new default.
- Tagged releases now carry prebuilt Windows downloads — an installer, a
  portable executable, an MSI package, and SHA-256 checksums — so the app no
  longer has to be built from source to be used.

### Changed

- Theme, sort order, path layout, compression, and gzip level are remembered
  between runs, so the app opens the way it was last left.
- Adding or removing a source no longer collapses the tree. Open branches and
  the selected row are restored afterwards, including when the new source
  changes where the root sits.
- An archive plan reopened from disk brings its compression and layout settings
  with it.

### Fixed

- Staging folders that sit under different top-level directories of one drive.
- The suggested output file name no longer comes out as `C__` when the staged
  paths span a whole drive.

## [1.0.0] - 2026-08-23

### Added

- Archive design view: a tree of staged folders and files with a checkbox, type
  icon, name, and right-aligned size on every row.
- Folders and files can be added by dragging them onto the window or through the
  toolbar. Anything added arrives collapsed and fully checked.
- The tree root is always the topmost folder common to everything staged, and
  re-roots itself as sources are added and removed. Directories between that
  root and a source appear as pass-through rows holding only the staged
  branches, never their unlisted siblings on disk.
- Each directory's loose files are grouped under a `<files>` pseudo-folder, so a
  folder holding thousands of files stays one row until it is opened. Unchecking
  the group drops every file in it and leaves subfolders alone.
- Partly-selected rows show `selected / total`, with a hairline beneath filled to
  that proportion so the split is readable without parsing the digits. The status
  bar restates the same ratio for the whole archive.
- Sorting by name or by size, ascending or descending. Names sort in natural
  order, so `file2` comes before `file10`. Size sorting uses each branch's size
  on disk, so rows hold still while boxes are being ticked.
- Archive plans save to JSON and reload. Unchecking a folder is recorded as a
  single rule covering that whole branch rather than a list of everything inside
  it, at `tree`, `files`, or `file` scope. Reloading rescans the sources and
  reports any rule that no longer resolves instead of dropping it silently.
- Build dialog showing the entry count, content size, and resulting archive size.
  For an uncompressed `.tar` that size is exact, computed from the real tar block
  layout; gzip keeps it as a labelled upper bound.
- Optional gzip compression with a level control, alongside plain `.tar`.
- Progress view with a bar, throughput, ETA, and a collapsible log that can be
  saved to a file.
- Keyboard navigation throughout: arrows to move and expand, space to toggle a
  row, `Ctrl+A` and `Ctrl+Shift+A` to check and clear everything.
- Light and dark themes, following the last choice made.

### Security

- The webview is granted no filesystem capability at all. Every scan, read, and
  write happens in Rust behind the command surface.

### Fixed

- Directory junctions and symlinks are recorded but never followed, so a folder
  containing a link to its own ancestor no longer scans forever.
- Paths beyond the legacy 260-character limit are scanned, archived, and read
  back correctly.
- A file that cannot be read is logged and skipped instead of ending the run, so
  an archive still completes when some of its contents are locked or missing.
- Cancelling a run deletes the partial archive rather than leaving a truncated
  file behind.
- Archive entries are named relative to the root's parent, so extracting produces
  one top-level folder instead of scattering files into the current directory.

[Unreleased]: https://github.com/dlvoy/tree-archiver/compare/v1.6.0...HEAD
[1.6.0]: https://github.com/dlvoy/tree-archiver/compare/v1.5.0...v1.6.0
[1.5.0]: https://github.com/dlvoy/tree-archiver/compare/v1.4.0...v1.5.0
[1.4.0]: https://github.com/dlvoy/tree-archiver/compare/v1.3.0...v1.4.0
[1.3.0]: https://github.com/dlvoy/tree-archiver/compare/v1.2.1...v1.3.0
[1.2.1]: https://github.com/dlvoy/tree-archiver/compare/v1.2.0...v1.2.1
[1.2.0]: https://github.com/dlvoy/tree-archiver/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/dlvoy/tree-archiver/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/dlvoy/tree-archiver/releases/tag/v1.0.0
