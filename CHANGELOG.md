# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Tagged releases now carry prebuilt Windows downloads — an installer, a
  portable executable, an MSI package, and SHA-256 checksums — so the app no
  longer has to be built from source to be used.

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

[Unreleased]: https://github.com/dlvoy/tree-archiver/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/dlvoy/tree-archiver/releases/tag/v1.0.0
