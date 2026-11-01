# Decisions

Choices made where the PRD is silent or was changed by agreement (PRD §0).

## D1. `h5tags-core` crate (2026-10-31, approved by the user)
Config loading, builds, and the tool logic shared by the CLI and the MCP server (search, browse, lists, paging, text/JSON rendering of results) live in a library crate `h5tags-core`. `h5-module` stays free of definition and XML code. The CLI and MCP crates are thin wrappers over it. PRD §13 updated.

## D2. Module version 23 supported from M1 (2026-10-31, approved by the user)
The Xbox One Guardians modules are version 23 (format-notes). `h5-module` reads both v27 and v23 from M1 on, choosing the layout from each file's version field. Multi-build features (`compare_builds`, cross-build `diff_tags`) stay in M6.

## D3. M1 tested on sample modules first (2026-10-31, agreed with the user)
The full Forge and Xbox installs aren't available yet. M1 is developed and tested against the sample modules listed in `CLAUDE.md`. The checks that need a full install run when it is available: every module in the deploy folder, the index speed targets (§6.2), and the user's 5 sample searches.

## D4. OS-agnostic (2026-10-31, requested by the user)
CLI and MCP server are supported on Windows, Linux and macOS. Only `xg.dll` de-tiling (backlog 8) is Windows-only, behind a cargo feature. Prefer pure-Rust dependencies. A GitHub Actions workflow builds and runs unit tests on all three OSes from M1. Integration tests against real game files run locally (the files can't go in CI). PRD §9 (M1), §13 and §14 updated.

## D5. Asset metadata always available, asset data only on request (2026-10-31, approved by the user)
Goal: the AI gets full information about every tag, including which assets exist, without paying for asset data it doesn't need. The index stores each tag copy's resources (kind from the file name, sizes) from the uncompressed file list, at no decompression cost. Output stays small: `brief`/`normal` show only `resources: N (size)` on the tag line, and `full` lists each resource. Reading a tag never decompresses its resource data. Asset contents are read only by calls that need them (`view_bitmap`, `export_bitmap`). The M1 integration test decompresses every file entry of every kind, to verify the reader. PRD §6.2, §6.4.4, §7.3, §8, §9 (M1), §12 (Q9) and `AGENT_GUIDE.md` updated.

## D6. Entry size and kind tools (2026-10-31, requested by the user)
The AI can get the kind and size of any file entry, alone or in bulk, without decompressing anything: `list_entries` (filter by tag, module, kind, minimum size; sort by size) and `list_resource_kinds` (counts and totals per kind). Both read only the index, so they're in M1 (CLI) and M4 (MCP). Sizes in text use decimal units with one decimal (`6.1 MB`); JSON gives bytes. PRD §6.3 and §9 (M1), and `AGENT_GUIDE.md` updated.

## D7. One definition per tool; CLI names derived from tool names (2026-10-31, approved by the user)
Each tool is defined once in `h5tags-core`: its name, its parameter struct and its implementation. The CLI subcommand and the MCP tool are both generated from that definition, so they can't drift apart. CLI name = tool name with `_` replaced by `-` (`search_tags` → `h5tags search-tags`); parameters become `--flags` the same way (`min_size` → `--min-size`). No aliases. PRD §14 example updated.

## D8. Index kept in memory, cached as compact binary; no database (2026-10-31, approved by the user)
While a process runs, the index lives in memory: a hash map from global ID to tag, a sorted path list for `browse` ranges, and plain scans for substring/regex search and filters. On disk it's cached in a compact binary format (a serde-based crate, checked for maintenance before adding) with a format version, so warm starts load in under a second. SQLite was considered and not chosen: substring search (`LIKE '%x%'`) scans every row in SQLite too; the data fits in memory; we don't need concurrent writes or ad-hoc SQL; and it's a C library, against the pure-Rust rule (D4). Revisit if the M5 header scan doesn't fit in memory, or if the M1 measurements miss the §6.2 targets.

## D9. Common filters across list tools; no query language in v1 (2026-10-31, approved by the user)
Complex questions are answered by combinable filters (AND) plus the agent chaining calls. `query`/`mode`/`class`/`kind`/`module`/`min_size`/`max_size`/`offset`/`limit` have one meaning everywhere (PRD §6.3 "Common filters"), implemented once. Added: `query`, `mode`, `max_size` on `list_entries`; `class` on `find_references`, where with `recursive=true` the walk follows every class but returns only the matching tags, flat, with depth and `via`. A general query language (SQL-like) was considered and left out: it's a bigger surface and the agent can write invalid queries. Add a filter when real use shows a gap. PRD §6.3, §6.6 and `AGENT_GUIDE.md` updated.

## D10. Copy identity from stored data; persistent results cache (2026-10-31, approved by the user)
`assetChecksum` may not reflect the stored data (AusarDocs), so copy identity compares the data itself: v27 by compression block checksums (free; already in the block list), v23 by compressed bytes or else decompressed header + tag data. Open question Q10 checks whether `assetChecksum` can be trusted. Expensive derived results (copy identity, header scan, definition check results) are kept on disk in a results cache keyed by the files they depend on (PRD §6.9), so work done once is reused by every later call and session. The cache only changes speed, never results; `definition_status` states how much has been checked. PRD §6.4.6, §6.5, §6.6, §6.9 and §12 updated.

## D11. Self-explanatory results; per-module index updates; targeted reindex (2026-10-31, approved by the user)
- Everything a tool can return is explained in `AGENT_GUIDE.md`, including what the agent may conclude from it. Markers and column names come from one central list, and a unit test fails if any is missing from the guide. Rare or partial results add one `hint:` line; common output carries no extra text.
- The index updates per module on startup (only new, changed or removed modules are re-read).
- `reindex(what=all|index|header_scan|results, module?, class?)` rebuilds only what is asked.
- PRD §6.2, §6.9, §9 (M4), §9a, backlog 7 and `AGENT_GUIDE.md` §4.6 updated.
