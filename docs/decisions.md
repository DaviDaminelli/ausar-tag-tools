# Decisions

Choices made where the PRD is silent or was changed by agreement (PRD §0).

## D1. `h5tags-core` crate (2026-10-31, approved by the user)
Config loading, builds, and the tool logic shared by the CLI and the MCP server (search, browse, lists, paging, text/JSON rendering of results) live in a library crate `h5tags-core`. `h5-module` stays free of definition and XML code. The CLI and MCP crates are thin wrappers over it. PRD §13 updated.

## D2. Module version 23 supported from M1 (2026-10-31, approved by the user)
The Xbox One Guardians modules are version 23 (format-notes). `h5-module` reads both v27 and v23 from M1 on, choosing the layout from each file's version field. Multi-build features (`compare_builds`, cross-build `diff_tags`) stay in M6.

## D3. M1 tested on sample modules first (2026-10-31, agreed with the user)
The full Forge and Xbox installs aren't available yet. M1 is developed and tested against the sample modules listed in `CLAUDE.md`. The checks that need a full install run when it is available: every module in the deploy folder, the index speed targets (§6.2), and the user's 5 sample searches.

Status (2026-11-01): the user's sample searches passed (`search-tags magnum --class weap`, `search-tags warthog`, `browse "objects\weapons\rifle"` and others, checked against the user's game knowledge). Still pending: every module of a full deploy folder and the §6.2 speed targets on a full install (when available), and the first CI run (when the repo is on GitHub).

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

## D12. Resource kinds are shown without markers (2026-10-31)
Resource file names end in `[index:kind]`, and some kinds carry trailing markers (`mesh resource!*`, `api resource*`). Their meaning is unknown (like definition markers, PRD §12 Q2). Output and the `kind` filter use the kind without trailing markers (`mesh resource`); the file name in the `name` column keeps them.

## D13. Class names from tag file names; definitions found by code, then by name (2026-10-31)
Verified on all samples: every tag's file name ends in `.<class name>`, and each class code always has the same name. Definition file names don't always spell the code (`(hsc_)lua.xml` is `hsc*`; `(mat)material.xml` is `mat `). So a class's name comes from its tags' file names (fallbacks: definition file name, then code), and its definition is found by code, then by class name. PRD §5.3 and §6.2 corrected.

## D14. Structural checks at index time; module status (2026-10-31)
The M1 cross-checks only read the uncompressed lists, so the index runs them on every module and keeps a summary. `list_modules` has a `status` column: `ok`, `!! incomplete file: <size> missing; N entries are past the end and can't be read`, `!! damaged: <first failed check>`, or `!! not indexed: <error>`, plus a `hint:` when any module isn't ok. Reason: a damaged or incomplete module file is then visible up front instead of surfacing later as confusing read errors. PRD §6.3 updated.

## D15. Default cache folder; relative config paths (2026-10-31)
`cache_dir` may be omitted: the OS cache folder is used (`%LOCALAPPDATA%\h5tags`, `~/Library/Caches/h5tags`, `$XDG_CACHE_HOME/h5tags` or `~/.cache/h5tags`). The CLI's `--cache-dir` overrides it. Relative paths in `h5tags.toml` are relative to the config file. Unknown keys in the config are an error (catches typos).

## D16. List result layout (2026-10-31)
Text: optional `build: <name>` line (more than one build configured), then `total: N` plus tool-specific summary values on one line (`total: 6380  total size: 1.3 GB`), then optional `in <prefix>:`, the tab-separated header and rows, the paging line, and `hint:` lines. JSON: `{ "build"?, "total", <summary keys>, "offset", "rows": [ {column key: value} ], "more"?: { "remaining", "next_offset" }, "hints"? }`; sizes are bytes; empty cells are omitted. Column headers and keys are defined in one list (`output::col`). `AGENT_GUIDE.md` §4.2 and §4.7 describe it.

## D17. Entries not stored in their module (2026-10-31)
Entries whose totals are 0 but whose sections and blocks describe data (format-notes "Entries not stored here") are kept in the index with their real size (sum of sections) and `stored = false`. `list_entries` shows `(not stored in this module)` in the `compressed` column, with a hint; reading such an entry is an error, never "0 bytes".

## D18. A class with no tags is an empty result, not an error (2026-10-31)
`search_tags(class=X)` and `list_entries(kind=X)` with no matching tags return 0 rows plus a `hint:`. If a definition exists for X, the hint says the class exists but its tags aren't in the indexed modules, and tells the agent not to claim the game has none. Otherwise it suggests checking the spelling.

## D19. `browse` details (2026-10-31)
Folders are matched case-insensitively and shown with the casing first seen (tags are sorted by path). Folders come first, then the tags directly in the folder; folder names end in `\`. The summary gives the folder, the number of subfolders and of tags directly in it. A folder with no tags under it is an error with a hint.

## D20. `reindex` in M1 (2026-10-31)
Only `what=index` (or `all`) does work in M1: it re-reads the matching modules even if unchanged. `header_scan` and `results` report "nothing to rebuild (not available in this version)" until those exist.

## D21. Layers inside `h5tags-core`; one definition per rule (2026-10-31, requested by the user)
Domain rules are kept apart from program inputs, and every rule is written once:
- **Input** (`tools/args.rs`): parameters shared by several tools (`BuildArg`, `PageArgs`), and turning raw arguments into domain values (`[build:]<tag>`, `query`+`mode`, `module`). Bad input becomes a tool error with a hint here, and only here.
- **Domain** (`catalog.rs`, `filters.rs`, `query.rs`, and the `h5-module` / `h5-defs` crates): typed inputs (`GlobalId`, `TagSpec`, `Filters`, `EntryScope`) and typed results (`TagLookupError`, `ClassLookup`, `ModuleStatus`). No text, hints or rendering.
- **Presentation** (`tools/present.rs`, `output.rs`): results and errors as the text the agent reads; markers and column names come from one list each (`output::marker`, `output::col`).
- Rules defined once: what a file entry is (`EntryKind::of`), file name conventions (`names.rs`, `EntryRecord::owner_name`/`tag_path`), where entry data ends (`Module::data_end`), check names (`checks::*`), global ID parsing and display (`GlobalId`), the common filters (`Filters::matches_tag`/`matches_entry`), the copy choice (`query::choose_copy`). Reports render text and JSON from the same items.
- Not followed from SOLID/clean-code checklists, deliberately: method and field count limits, "no `else`", polymorphism instead of `match`, and newtypes for every number. Newtypes are used where values could be mixed up (global IDs, class codes, sizes).

## D22. Overview and explanations for searches (2026-10-31, approved by the user)
Found by trying the CLI as an agent would. A broad `search_tags` (230 hits for `assault_rifle`, 100 of them materials) gave no overview, and a typo returned a bare `total: 0`. Now:
- When the hits span more than one class, the summary line adds `by class: …` (top 8, most first, then `+N more`; JSON: `by_class: { items: [{ name, count }], more }`). About 30 tokens, and it lets the agent narrow with `class=` in one call.
- An empty result gets a `hint:`: either that the class/module filters removed N tags the query alone matches, or up to 5 similar folder/file names from tag paths (normalized Damerau-Levenshtein ≥ 0.7, `strsim`, the crate PRD §13 lists for did-you-mean), or a next step.
PRD §6.3 and `AGENT_GUIDE.md` §4.6 updated.

## D23. A `table` format for people (2026-11-01, approved by the user)
The AI's `text` format is tab-separated (fewest tokens), which looks jagged in a terminal because tab stops are 8 columns. The CLI gets a third rendering, `table`, from the same data (D21): the same lines as `text` (summary, `in <prefix>:`, paging, hints), but rows space-padded with two spaces between columns, numeric columns right-aligned, and the long name/path column moved last (`Table::wide_column`) so the short columns line up whatever the path length. Paths are never cut. CLI default: `table` in a terminal, `json` when stdout is piped or redirected (`std::io::IsTerminal`, all OSes). MCP stays `text`; the guide is unchanged because the AI never sees `table`. Reports and errors render the same in `table` and `text`.

## D24. Colors in the terminal table (2026-11-01, approved by the user)
In the `table` format, when the output is a terminal: the part of each name that matched the query is highlighted (bold yellow), headers are bold, `!!` lines and cells red, hints cyan, the paging line dim. The matcher reports where it matched (`NameMatcher::find`, domain); the table renderer turns those ranges into colors (presentation), carrying them through the `in <prefix>:` cut (a match inside the prefix is highlighted on the `in` line). Colors are added after padding, so alignment is unaffected. `text`, JSON, piped output and MCP never contain color codes; a test checks that the colored table equals the plain one once the codes are stripped. Color is on or off, nothing else (no "always": colors outside a terminal are useless). Precedence: `--no-color`, then the preferences file, then `NO_COLOR`, then on. `anstyle`/`anstream` (already in the tree via `clap`) produce the codes and handle older Windows consoles.

## D25. Preferences file (2026-11-01, approved by the user)
Per-person CLI display settings live apart from `h5tags.toml` (which describes builds): an optional `preferences.toml` in the user config folder, `--preferences <file>` to override. First key: `color = true|false`. `h5tags` never creates or changes it; unknown keys or wrong types print a warning naming the file, and defaults are used. The file is read only when colors are possible (`table` format, a terminal, no `--no-color`), at most once per run; the MCP server never reads it.
Amended the same day (requested by the user): the file can be managed from the CLI instead of by hand. `h5tags preferences` shows the file location and each value with its source (file or default); `preferences set color on|off` (also true/false, yes/no) and `preferences unset color` write it, creating the folder if needed. This command is the only writer; it rewrites the file whole, with a header saying so. It is CLI-only (not a tool, like the developer commands of PRD §6.7) and runs before any config or game files are loaded.

## D26. No wasted work in per-item loops (2026-11-01, requested by the user)
Loops over all tags or entries (tens of thousands per call) allocate nothing per item: the default-mode query is compiled once into a case-insensitive literal search; `exact` compares the display name as `<path>.<class>` in parts instead of building it; the module filter and the resource kind compare borrowed text; `browse`, `list_resource_kinds` and the did-you-mean index key their maps with text borrowed from the catalog (`path_lower`); `list_entries` filters while iterating and sorts with one key per match; `class_name` borrows. Measured on the Forge sample (62,411 tags, 87,221 entries, release build): `search_tags` 14.7 → 5.0 ms, `browse` 7.4 → 2.7 ms, `list_resource_kinds` 3.2 → 1.2 ms, `list_entries` 1.3 → 0.25 ms. Recorded as a code convention in PRD §13.

## D27. A progress spinner for slow calls (2026-11-01, approved by the user)
Queries take milliseconds, but a cold index build of a full install, `reindex` and the M5 header scan can take seconds to minutes. The CLI shows a spinner and a short message on stderr (`⠹ forge: reading modules 120/340`, then `running <tool>`), only after 0.3 s so fast calls never flicker, and clears the line before printing results. Shown only when stderr is a terminal; off with `-v` (log lines would interleave), `--no-progress` or the `progress = false` preference. Log output goes through the spinner and clears its line first, so the two never mix. Layers: `Index::update_with_progress` reports `Progress { stage, done, total }` from its worker threads (domain); the session tags it with the build name and passes it to a sink set by the caller (`Session::open_with_progress`); only the CLI draws it. The MCP server will pass no sink. About 150 lines of standard Rust, no new dependency. PRD §6.1 updated.

## D28. Reading a tag copy refuses a module changed since indexing (2026-11-01)
The PRD doesn't say what happens when a module file changes between indexing and reading. The index stores each entry by its number in the file list, so a replaced file could make that number point at another entry. Before reading a tag copy, `read_stored_tag` compares the file's size and modification time with the index's stamp (the same stamp `reindex` uses). If they differ, the read fails with `<module> changed since it was indexed` and the hint `run reindex`. Nothing is read from a changed file. It decompresses only the header and tag data sections (PRD §6.2). Every command that reads tag data uses it (`header-dump` now, `read_tag` from M3 on).

## D29. `header-dump` layout (2026-11-01, plan approved by the user)
One titled table per list of the tag header (PRD §6.7), through a new `Output::Parts`: `## <title>` above each table in `table`/`text`, and one key per part in JSON. The tables and their columns come from the same data in every format. The columns are local to the command and are not explained in `AGENT_GUIDE.md`: the command is CLI-only, and the AI never sees it.
- **Display names:** a referenced tag's display name comes from the index when its global ID is indexed. Otherwise it is the name listed in the tag + `.` + the class name, or the class code when the class is unknown. Listed names never carry the class suffix, and every dependency has one (format-notes "Tags").
- **Tag references** show what their field holds (global ID and class), not just the dependency index, because a reference without a dependency can still name a tag (PRD §5.2). Null references (global ID -1) have no display name.
- **Raw values:** hashes are shown as 8 hex digits (string IDs with their MurmurHash3 value, PRD §5.2), offsets in hex, and indices of -1 as empty cells (omitted in JSON). Fields of unknown meaning are shown under their offset name (`unknown4`), since the command exists to investigate them.
- The `module=` copy choice and its "other copies" hint are shared with `list_entries` (`args::tag_copy`).
- **Zonesets** (added after the user asked what the unknowns were, 2026-11-01): names are resolved from the tag's own string_ids (hash shown too). A `## zoneset resources` part lists each zoneset's resource indices with the resource entry's kind and name. Zoneset tags that aren't indexed are named from the tag's dependency list, by the same display name rule. The resolution (`TagHeader::string_id_names`) is the domain rule M3 will use for string_id fields.
- **Field path** (requested by the user, 2026-11-01; built in M3): when a definition exists, the tag references part gains a field path column (`barrels[0].projectile`), next to the raw columns.

## D30. The guide explains every tag internal the AI meets (2026-11-01, requested by the user)
The tag header's lists (dependencies, tag references, string IDs, data blocks, tag structs...) are what tools are built on, and from M3 on, parts of them reach the AI: reference targets, null references, references without a dependency entry, string IDs resolved from the tag's string table, header ↔ definition mismatches. Each time a tool starts showing one, `AGENT_GUIDE.md` gains a plain-words explanation of what it is and what it says about the tag, in the same commit. Explanations are written with the output they explain, not ahead of it, so the guide never describes output that doesn't exist. `header-dump` stays CLI-only (D29). PRD §9a updated.

## D31. Research unknown fields with a survey test (2026-11-01, requested by the user)
Decoding the zoneset fields took seven guess-edit-run rounds over the real files, each costing session time. Next time an unknown field matters, the first step is a reusable **survey test**: one ignored real-file test that reports, in a single run over every tag, each field's distinct values (overall and per class), whether it is constant per class, its relations to other fields (equality, count or offset of something), and whether it equals a hash of something known. Hypotheses are then checked against that report, not one by one. It is not built yet: the remaining unknowns (PRD §12 Q13) aren't needed by any milestone, so we wait until one is. Recorded in PRD §12 ("Research method").

## D32. GUID check stays `warn` by default (2026-11-01)
The PRD planned to switch `guid_check` to `enforce` once Q3 (byte order) was answered. The M3 survey answered it: `srcguid` is a Windows GUID. But it also showed that 0.2% of real tag blocks (3,840 Forge, 2,483 Xbox, in 11–12 classes, mostly collision models) carry a struct GUID that appears in no definition, while the definition places a block at that field and the sizes fit. The reader then compared GUIDs field by field and found more: 34,121 mismatches on Forge (46,993 Xbox), at only 64 fields (60), mostly because the definition names another struct type of the same layout (format-notes "GUIDs"); every one of those structs fits its size and passes the header ↔ definition cross-check. Enforcing would hide values the size check accepts. So the default stays `warn`: the values are shown with `warning: definition GUID mismatch`, and the size check (always enforced) remains the guard against misreading. `enforce` is still available in the config. PRD §6.4.5 and §12 Q3 updated.

## D33. `definition_status` keeps the definition gap list (2026-11-01, requested by the user)
New or changed tag layouts should be tracked so their definitions can be written or dumped later. The M3 survey found three kinds of gaps: classes with no definition (none in the samples yet), classes whose layout differs (5 Xbox classes), and struct GUIDs in no definition (3,840 Forge and 2,483 Xbox tag blocks). `definition_status` (M5, with the results cache) reports all three per class, with field paths, plus each class's main struct GUID per build, and can export the list as JSON. Dumping definitions is a research item in the PRD backlog (§10 item 10); its output must pass the same survey and checks. PRD §6.5 and §10 updated.

## D34. Names and customs as the real definitions write them (2026-11-01)
PRD §5.3 described a simpler name syntax than the 68,781 real names use (format-notes "Names", "Custom fields"). Choices where the PRD was silent:
- **`X&Y`:** the display name is `X`, the developer name (PRD §4: field names are the original developer names). `Y` is kept as the field's label. Paths use `X`. Making `Y` searchable or shown is for M3's output step.
- **`{old name}`:** kept, so `search_definitions` can find a field by an old name (`Radius{Offset}` is found by "offset"), but not part of the display name or of paths.
- **`|hint`:** an editor colour or style, of no use to readers; dropped.
- **Markers:** only the trailing run counts, so literal `&`, `?`, `!` inside names stay text.
- **Group headings** (`_field_custom` that is neither a section label nor a widget, 533 of them): shown as `###` headings, like an explanation without text, so the AI sees the editor's grouping and `section=` can select them. The PRD had them ignored as widgets.
PRD §5.3 updated.

## D35. Paths and sections with real names (2026-11-01)
PRD §6.4.2 splits paths at `.` and `[`, but 282 real names contain those characters. Choices:
- **Path steps** match the names at their level: the longest name the remaining text starts with (case-insensitive), followed by the end, `.`, or `[n]`/`[*]`. All 36,450 field paths of the definitions round-trip with this rule.
- **Passing through a tag block without an index** uses entry 0, as a block shown without an index expands entry 0 (PRD §6.4.2).
- **Sections** end at the next heading of the same or a higher rank among the same fields. Headings are class sections (rank 2), explanations and group headings (rank 1). Structs and tag blocks don't end a section: they are fields of it. A section label below the top level counts as a group heading.
- The path and section rules live in `h5-meta` (`path.rs`), shared by `read_tag`, `get_definition` and `outline`. PRD §6.4.2 and §6.4.3 updated.

## D36. How `read_tag` lays out a tag (2026-11-01, approved by the user)
The output follows PRD §7 and AGENT_GUIDE §4, with these choices, agreed on a real sample (the assault rifle):
- **Columns:** `text`, what the AI reads, separates columns with exactly two spaces and doesn't pad them, since padding costs tokens and adds nothing for the AI. `table`, for people in a terminal, aligns the name and type columns within each run of fields under one heading. Same rule as lists (D23). PRD §7.2 updated.
- **Indentation** is nesting: the main struct's fields have none; each struct, array or tag block entry adds 2 spaces, for headings and fields alike. A line belongs to the nearest heading above it with less indentation.
- **Entry labels from a tag reference** use the target's file name (`unsc_ads_default.effect`), so block headings stay short; the field line shows the full name.
- **Numbers** are exact: the shortest form that reads back to the stored value (`0.1708791`, `1`), never rounded.
- **An explanation with no text whose title is the next field's name is not shown**: 103 of the 547 textless explanations do this (`firing` before the `firing` struct), and the field's own heading already says it. Same idea as custom widgets (D34).
- **A tag reference with no listed name and no index entry** shows its global ID and stored class.
- **`[*]` reads** put each field's options once in a `legend:` block (AGENT_GUIDE §4.4); JSON keeps them on every field.
- Copy identity is computed on each call (block checksums; PRD §6.4.6) until the results cache (§6.9, M5).
