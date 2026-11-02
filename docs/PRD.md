# PRD: `h5tags` — Halo 5 tag analysis tool and MCP server (Rust)

**Status:** Draft v2 · **Owner:** David · **Date:** 2026-10-31
**Companion document:** `docs/AGENT_GUIDE.md` (normative for output format; see §9a)

---

## 0. How to read this document

- **MUST** = required. **SHOULD** = expected unless there is a stated reason not to. **MAY** = optional.
- Section numbers are referenced as §N. Functional requirements have IDs (`FR-n`). Milestones are `Mn`.
- Facts in §5 come from research (AusarDocs, Lord Zedd's H5Tags, observing Reclaimer's behaviour). Each MUST be verified against real game files when first implemented. If a fact turns out wrong, fix the code **and** update §5 and `docs/format-notes.md`.
- Anything this document does not specify: choose the simplest behaviour consistent with the rest of the document, and record the decision in `docs/decisions.md`. Do not invent features.
- Examples (tag paths, IDs, values) are illustrative. They are not real data.

## 1. Summary

`h5tags` reads Halo 5 `.module` files and exposes their tags to an AI agent (Claude, over MCP) and to a human (over a CLI). The agent can search, browse, read, compare and follow references between tags. It gets everything Reclaimer's meta viewer shows, plus what Reclaimer leaves out: enum labels, flag names, every available option, units and help text from the definition XMLs, and clear reasons whenever a value is empty or unreadable.

Supported game builds, in order of priority:

1. **Halo 5: Forge** (Windows PC). Primary target.
2. **Halo 5: Guardians** (Xbox One). Same engine, more content (campaign, etc.). Added at M6.

Deliverables, all built from one Rust workspace:

- **`h5tags`**: a CLI. Default output: JSON.
- **`h5tags-mcp`**: an MCP server over stdio, launched locally by Claude Code / Claude Desktop. Default output: compact text (§7).

## 2. Scope

### 2.1 Version 1 = milestones M1–M7 (§9)

- Index and search all modules of one or more builds.
- Browse tag paths like folders.
- Read tag fields with names, types, values, units, help text, options and resolved references.
- Read definitions (schemas) without tag data; search definitions.
- Outline large tags by section.
- References (outgoing, incoming, recursive), field comparison across a class, tag diffs, string search, tag copies across modules, map load manifests.
- Several builds side by side, with cross-build comparison.
- Bitmap viewing and export for **untiled** textures (Forge PC).
- Agent instructions (server instructions, guide resource, tool descriptions).

### 2.2 Not in version 1

- Editing or writing modules (§11).
- Model, animation, sound or BSP export.
- Reading any resource type other than bitmap data.
- De-tiling Xbox One textures (§10, backlog).
- Halo Infinite support (keep the code open to it; don't build it).
- A GUI.
- Reading live game memory (out of scope permanently).

## 3. Legal and licensing constraints (MUST follow)

- **Reclaimer (GPL-3.0) is a reference, not a source.** MUST NOT translate, port or closely mirror its code or code structure. Implement from AusarDocs, from the facts in §5, and from our own checks against real files. Reclaimer is not used for testing (§9).
- MUST NOT copy Reclaimer's data files (e.g. `Halo5FieldDefinitions.xml`). We write our own field type table (§6.4.4).
- MUST NOT bundle Lord Zedd's definition XMLs (https://github.com/Lord-Zedd/H5Tags). The user downloads them and configures the path to their `tags/` and `tag_resources/` folders.
- MUST NOT bundle any game file or Microsoft DLL (`xg.dll`, `kernelx.dll`). The user configures paths to their own copies.
- MUST keep `docs/format-notes.md`: for each piece of format knowledge, where it came from (AusarDocs section, H5Tags observation, our own test on file X).

## 4. Glossary

Use these terms consistently in code, docs and output. **"Block" alone is ambiguous: always qualify it.**

| Term                  | Meaning                                                                                                                          |
| --------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| **Build**             | One version of the game, with its own deploy folder. Named in config (e.g. `forge`, `xbox`).                                     |
| **Module**            | A `.module` archive file in a build's deploy folder.                                                                             |
| **File entry**        | One entry in a module's file list. Can be a tag, a resource, or a non-tag file (e.g. `loadmanifest`).                            |
| **Compression block** | A piece of a file entry's data, compressed separately with zlib (module level).                                                  |
| **Tag**               | A file entry with a global ID ≠ -1: one piece of game data.                                                                      |
| **Global ID**         | 32-bit tag identifier, the same in every module. Written as `0x` + 8 uppercase hex digits: `0x00A1B2C3`.                         |
| **Class**             | A tag's type: a 4-character **class code** (`weap`) and a **class name** (`weapon`). AusarDocs calls the code "group tag".       |
| **Tag path**          | The tag's name, e.g. `objects\weapons\rifle\assault_rifle\assault_rifle`. Separator `\`.                                         |
| **Display name**      | `tag path` + `.` + `class name`, e.g. `...\assault_rifle.weapon`.                                                                |
| **Copy**              | The same tag (same global ID) stored in more than one module.                                                                    |
| **Definition**        | One Lord Zedd XML file describing a class's fields.                                                                              |
| **Field**             | One XML element in a definition, and its value in a tag.                                                                         |
| **Tag header**        | The metadata header at the start of each tag's data (magic `hscu`).                                                              |
| **Data block**        | A region of a tag's data, listed in the tag header's data block list (tag level).                                                |
| **Tag struct entry**  | An entry in the tag header's tag struct list.                                                                                    |
| **Struct field**      | `_field_struct`: a fixed group of fields, stored inline.                                                                         |
| **Tag block field**   | `_field_block_64`: a list of **entries**, each with the same fields. `barrels[0]` is entry 0 of the `barrels` tag block field.   |
| **Array field**       | `_field_array`: a fixed-size repeated group stored inline.                                                                       |
| **Section**           | A heading in the output, produced by an explanation field, a top-level custom label, a struct field or a tag block field (§7.3). |
| **Class section**     | A top-level section marking fields inherited from a parent class (e.g. `OBJECT`, `DEVICE`).                                      |
| **Field path**        | Address of a field inside a tag (§6.4.2).                                                                                        |

## 5. Format facts

### 5.1 Modules (AusarDocs `FileFormats/Module.md`, version 27)

- All data is little-endian.
- Header: magic `'dhom'` at +0x00, **version** at +0x04. Four-character codes (the magic, class codes) are little-endian `u32`s: the magic's bytes on disk read `mohd`. Known versions: **23** (Halo 5: Guardians, Xbox One; verified on real files; Reclaimer labels it "Halo 5 Server"), **27** (Halo 5: Forge, documented by AusarDocs; verified), 53 (Halo Infinite, not supported).
- Layout sizes depend on the version. MUST pick layouts from each file's version field, never from the build name.
  - Version 27: header 0x38 bytes; file entry 0x58 bytes; compression block entry 0x20 bytes.
  - Version 23: header 0x30 bytes (same fields as v27 up to +0x2F, no checksum); file entry 0x58 bytes (same fields as v27); compression block entry 0x14 bytes (compressed offset, compressed size, uncompressed offset, uncompressed size, `compressed` as u32; no checksum). Verified on two Xbox modules (`docs/format-notes.md`).
- Order after the header: file list → file name table (null-terminated ASCII strings; each file entry has an offset into it) → resource list (int32 file indices) → compression block list → data area. No padding between them. The header, file list, file name table, resource list and block list are stored **uncompressed**, so listing a module's files never needs decompression.
- File names: a tag's file name is `tag path` + `.` + `class name`; a resource's is its owner's file name + `[index:kind]` (e.g. `[0:mesh resource!*]`). Verified on v23 and v27 (every tag and resource name in the sample modules).
- File entry (v27) holds, among others: name offset, parent file index (for resources, -1 otherwise), resource count + first resource index, compression block count + first block index, data offset (relative to the data area), total compressed/uncompressed sizes, flags, **global ID** (-1 if not a tag), asset ID, asset checksum, **class code** (group tag), and the uncompressed sizes and compression block counts of three sections: header, tag data, resource data.
- File entry flags: bit 0 `Compressed`, bit 1 `HasBlocks` (if unset, the file has exactly one compression block, described by the entry itself), bit 2 `RawFile` (not split into header/tag data/resource sections; read whole).
- File entry alignment fields (header/tag data/resource data alignment) add **no padding** to the stored data: the three sections are stored back to back (verified on v23).
- Resource file entries have global ID -1 and class code -1, so they are non-tag files (verified on v23).
- Compression block entry (v27): checksum, compressed offset (relative to the file's data start), compressed size, uncompressed offset, uncompressed size, `compressed` flag. If `compressed` is false, copy the bytes as-is. Compression is **zlib** (use `flate2`); streams seen so far start `48 0D` (4 KiB window). No Oodle and no external DLL.
- Modules can contain **non-tag files** (global ID -1). Map modules in the `any` platform folder contain a `loadmanifest` file: an array of global IDs of the tags the map loads; the count is the header field `numLoadedTags`.
- Modules are designed to be layered ("module stack"). So the same tag can have copies in several modules. Which copy the game uses at runtime is not documented (§12, Q5).

### 5.2 Tags (AusarDocs `FileFormats/CachedTag.md`)

- Tag data starts with the **tag header** (magic `'hscu'`, 0x50 bytes), followed by these sections in order; each starts where the previous ends: dependency list, data block list, tag struct list, data reference list, tag reference list, string_id list, string table, zoneset info, tag data, resource data. AusarDocs gives each entry layout; implement from it. Verified on every sample tag (`docs/format-notes.md` "Tags"): version 27 in both builds; data blocks all lie in the tag data section; the zoneset info has the documented layout, and each zoneset entry also lists the tag's resources it loads (+0x08 count, +0x10 offset of `uint32` indices into the file entry's resource list; not in AusarDocs). Zoneset names are string_ids from the tag's own list. **Not in AusarDocs:** in about 12% of tags (materials, particles, decals, `paas`, `sbsp`…) the header section continues after the zoneset info with **trailing data** of unknown meaning (§12 Q11); its size is the remainder of `headerSize`. Keep it as raw bytes.
- **Dependency list:** one entry per referenced tag: class code, name offset (into the string table), asset ID, global ID.
- **Data block list:** size, section (0 header, 1 tag data, 2 resource data), offset within that section.
- **Tag struct list:** per struct instance: a 16-byte **type GUID**, a type (0 main, 1 tag block, 2 resource, 3 custom), a **target index** (the data block holding the struct's data; -1 = null), and the **(field data block, field offset)** of the field that points to it. The main struct entry has type 0; its target data block holds the tag's top-level fields.
- **Data reference list:** per `_field_data_64`: target data block (-1 = null) and the field's (data block, offset).
- **Tag reference list:** per tag reference field, including null ones: the field's (data block, offset), the target's name offset in the string table, and its dependency index. This gives every reference's exact location **without any definition XML**. **Dependency -1 does not mean null** (verified, `docs/format-notes.md` "Tags"): about 8% of these references still name a tag (global ID, class and usually a listed name in the field and string table) that the dependency list leaves out. A reference is null when its field's global ID is -1. So the dependency list is not the complete list of what a tag references.
- **String table:** null-terminated strings: referenced tag names and every string_id string used by the tag. The text is **Windows-1252**, not UTF-8 (a few strings hold `…`, `’`, `é`). A string_id hash is computed over the stored bytes.
- **Pointers are never stored in tag data.** Tag block entries, data references and resources are located only through the tag struct list and data reference list, matching on (data block, field offset).
- Field layouts:
  - Tag block field: 0x1C bytes; **entry count is int32 at +0x10**.
  - Data reference field: 0x1C bytes; **size is uint32 at +0x18**.
  - Tag reference field: 0x20 bytes; uint32 at +0x08 = byte length of the listed name (0 when none); **global ID int32 at +0x0C**, class code at +0x18. With a dependency, the global ID and class equal the dependency's.
  - Pageable resource field: 0x10 bytes.
  - string_id field: 4 bytes, a **MurmurHash3 x86 32-bit** hash of the string (seed 0; verify: `"__default__"` → `0x9B555AD2`). Resolve by hashing every string in the tag's own string table. No external string list is needed.

### 5.3 Definitions (Lord Zedd's H5Tags)

- Two folders:
  - `tags/`: 289 class definitions, file name **`(code)class_name.xml`**, e.g. `(weap)weapon.xml`, `(_fx_)sound_effect_template.xml`. The file names are **almost** the class code → class name mapping: codes can contain `_`, `!`, spaces or other symbols; parse the code as the text between the first `(` and `)`. Two caveats (verified, `docs/format-notes.md`): trailing spaces are trimmed (`(mat)material.xml` is class `mat `; pad codes to 4 characters with spaces), and characters not allowed in file names are replaced by `_` (`(hsc_)lua.xml` is class `hsc*`). So match a class to its definition by code first, then by class name.
  - `tag_resources/`: 18 resource layouts, file name `(NNNN)name.xml`, e.g. `(0008)bitmap_data_resource_definition.xml`. See §5.4.
- Root element `<plugin>`. Each child element is a field; **the element name is the field type** (`_field_real`, `_field_block_64`, ...). 61 distinct types are used (verified, `docs/format-notes.md` "Definitions"); the code MUST support every one (generate the list with a script at build time and fail tests if one is missing).
- Field offsets: a field's offset is the sum of the sizes of the fields before it in the same struct. A tag block entry's size and a struct field's size are the sum of their children. Zero-size fields (`_field_explanation`, `_field_custom`) take no space.
- **Array fields have no count attribute** in the XML. Treat each array field as **one** instance of its children (size = sum of children), the same as Reclaimer. Flag this in `docs/format-notes.md`. Open question §12, Q7.
- Attributes, matched **case-insensitively** (fields use `name`, enum/flag items mostly use `Name`):
  - `name`: field name with markers (below).
  - `validtags` (tag reference fields): allowed target class codes, comma- or space-separated; `*` is a wildcard (e.g. `hsc*`).
  - `max` (tag block fields): maximum entry count.
  - `length` (`_field_pad`, `_field_skip`): size in bytes.
  - `targetName` / `targetGuid` (block index fields): the tag block that the index points into (match `targetGuid` against tag block fields' `srcguid` in the same definition).
  - `srcName` / `srcguid` (struct and tag block fields): original struct name and type GUID.
- Enum and flag options are `<item Name="...">` children. **An option's value (enum) or bit number (flags) is its position among the `<item>` children, starting at 0.**
- **Name syntax** (extended in M3 from all 68,781 real names, `docs/format-notes.md` "Names"). A `name` attribute is: `display name` + optional markers + optional `&label` + optional `{old name}` (repeatable) + optional `|editor hint` + optional `:units` + optional `#help`. Parse in this order:
  1. `#` and everything after it is **help text**.
  2. Every `{…}` group is an **old name** (an earlier or alternate name); remove it. Braces can hold `:` and `|` and can follow the units, so they come out before steps 3–4.
  3. `|` and everything after it is an **editor hint** (a colour or style); drop it.
  4. In what remains, `:` and everything after it is **units**. If units start with `[min-max]` (also `[0,1]`, `[0,+inf]`), that is the **range**; the rest is the unit (`auto-exposure delay:[0.1-1]seconds` → range `0.1-1`, unit `seconds`).
  5. An `&` with no space on either side splits the name from a **label** (`size class&Analytic Lightmap Size`). The display name is the part before it, the developer name (decisions D34); a spaced ` & ` is text.
  6. The **trailing run** of `^ * ! ~ &` characters is the **markers**; the rest, trimmed, is the display name. Marker characters elsewhere are text. Keep the markers as a string.
  - `^` = this field labels its tag block's entries (confirmed by Reclaimer's behaviour).
  - `*`, `!`, `~`, `&` = **meaning unconfirmed** (§12, Q2). They appear on runtime/internal fields (`global tag ID!`, `vtable space*~!`, `unused label!`) and on unused options (`bit3!`, `(unused2)!`). **Provisional rule:** a field or option whose markers include `!` is **hidden**. Implement this rule in one place so it's easy to change.
  - Item names use the same syntax (help text after `#` is common on items).
- **Sections** in definitions (all zero-size, in field order):
  - `_field_explanation`: `name` = heading (can be empty); element text content = explanation (can be multi-line or empty).
  - `_field_custom`: a **section label**, a **group heading** or an **editor widget** (verified, `docs/format-notes.md` "Custom fields"). Rule, in this order: a custom with no name, with the same raw name as the next field (a control for that field), or starting with the word `import` or `show` (a tool button) is a **widget** and is ignored. Otherwise, a name that (after removing `$` and spaces at the ends) is all uppercase letters, digits, spaces and `_` is a **section label** (`OBJECT`, `$$$ DEVICE $$$` → `DEVICE`, `IMPACT AUDIO`). Any other name is a **group heading** (`Outlining`, `Unit Camera`): it heads the fields after it, like an explanation without text (§7.3). Section labels **at the top level** of a definition are **class sections** (inherited parent classes, e.g. `dspn` = `OBJECT`, then `DEVICE`, then `DISPENSER`).
- **Common structs** (identified by `srcguid`):
  - `151d0146-479c-181f-3970-ec869043bc33` "internal struct" (global tag ID + local handle): at the top of every definition, together with the `vtable space` field before it. Always hidden.
  - `5a14a98f-4484-53fa-d468-10809e45f10a` mapping function (529 uses): contains only a `_field_data_64`. Shown as one line (§7.2).
- Largest definitions: `scnr`, `effe`, `vehi`, `bipd`, `char`, `gint`, `unit`, `weap`. Nesting goes at least 6 levels deep (`dspn`).

### 5.4 Tag resources

- Large data (bitmap pixels, geometry, animations, BSP data, video, shader bytecode, ...) is stored outside the tag's main data: as resource file entries in the module, owned by the tag (file entry `firstResourceIndex` / `resourceCount`; the resource's `parentFileIndex` points back), and/or in the tag's resource data section. Tag fields point to it with `_field_pageable_resource_64`.
- `tag_resources/*.xml` describe resource layouts. The numeric prefix appears to be a resource type index; how a resource identifies its type is unknown (§12, Q6). In v1, only the bitmap layout (`(0008)bitmap_data_resource_definition.xml`) is used, and only for bitmaps (§6.8).

### 5.5 Reclaimer behaviour (background only; not used for testing)

- **Tag search** ("Search Tags" box) filters tags, not files. A tag matches if its tag path **contains** the query (case-insensitive), **or** its class code **equals** the query, **or** its class name **equals** the query (both case-insensitive). It searches only the opened module plus manually linked ones, shows one row per global ID, and skips global ID -1.
- **Meta viewer** loads `(code)name.xml` for the class. If one top-level field fails to read, it **stops reading the rest of the tag**; failed values and empty tag block fields both just appear greyed out. Its JSON export writes enums and flags as plain numbers (no labels or options).
- **Bitmap viewer** shows the texture with toggles per channel and the format name; exports DDS, PNG, TIF, TGA, with optional per-channel files.

## 6. Functional requirements

### 6.1 Configuration and builds (`FR-0`)

Config file `h5tags.toml` (path given with `--config`; default: next to the executable):

```toml
cache_dir = 'C:\h5tags\cache'
notes_file = 'C:\h5tags\h5tags-notes.toml'   # used from the backlog (§10); MAY be omitted in v1
tag_cache_mb = 256                            # decompressed tags kept in memory (§13); default 256
large_read_tokens = 3000                      # size guard on whole reads (§6.4.1); default 3000

[[build]]
name = "forge"                                # [a-z0-9_-]+, unique
deploy = 'C:\...\Halo 5 Forge\deploy'
defs = 'C:\...\H5Tags\tags'
resource_defs = 'C:\...\H5Tags\tag_resources'

[[build]]
name = "xbox"
deploy = 'D:\h5_xbox\deploy'
defs = 'C:\...\H5Tags\tags'                   # same XMLs unless a build-specific copy is needed
resource_defs = 'C:\...\H5Tags\tag_resources'
```

- Shortcut for one build without a config file: `--deploy <path> --defs <path> [--resource-defs <path>]` creates a single build named `default`.
- **Preferences** (CLI display only, per person, D25): an optional `preferences.toml` in the user config folder (`%APPDATA%\h5tags`, `~/Library/Application Support/h5tags`, `$XDG_CONFIG_HOME/h5tags` or `~/.config/h5tags`; `--preferences <file>` overrides). Keys, all on/off: `color` (colors in the terminal) and `progress` (the progress spinner), both default `true`. Managed with the CLI-only command `h5tags preferences` (show the file and values), `preferences set <key> on|off`, `preferences unset <key>`; that command is the only thing that writes the file (it rewrites it whole). Editing it by hand also works; unknown keys are reported. It is read at most once per run, and only when a setting can matter (colors: `table` output to a terminal, no `--no-color`; progress: stderr is a terminal, no `--no-progress`, no `-v`).
- **Progress indicator** (CLI only, D27): when a call takes longer than 0.3 s, a spinner with a short message (`⠹ forge: reading modules 120/340`) is drawn on stderr, and cleared before the results are printed. Only when stderr is a terminal; never with `-v`, `--no-progress` or `progress = false`. Results on stdout never contain it; the MCP server never shows it. The index reports progress through a callback (domain); only the CLI draws it.
- The **first** build listed is the default build.
- Every tool that reads tags takes an optional `build` parameter (default: the default build). Every result states its build when more than one build is configured.
- **Tag argument syntax** (all tools): `[build:]<tag>`, where `<tag>` is one of: a global ID (`0x00A1B2C3`, case-insensitive), a tag path, or a display name. Examples: `0x00A1B2C3`, `xbox:objects\weapons\rifle\assault_rifle\assault_rifle`, `forge:...\assault_rifle.weapon`. A `build:` prefix overrides the `build` parameter. If a path matches several tags (same path, different classes), return the candidates and don't read any.
- Module paths in output are relative to the build's deploy folder (e.g. `any\levels\...\foo.module`), so platform subfolders stay visible.

### 6.2 Index (`FR-1`)

- On startup, for each build, load the index from `cache_dir` and bring it up to date **per module**: modules that are new, changed (size or modification time) or removed (recursive under `deploy`) are re-read or dropped; unchanged modules load from the cache. With no cache, every module is read.
- Building the index reads only module headers, file lists and file name tables (no tag data). It stores, per build:
  - per global ID: tag path, class code, class name, and the list of copies (module, file entry index, asset checksum, sizes, and the copy's **resources**: file entry index, kind, compressed and uncompressed size). The kind comes from the resource's file name (`…[0:mesh resource!*]` → `mesh resource`). This is metadata only: nothing is decompressed;
  - per module: path, version, file count, tag count, and whether it has a `loadmanifest`.
- Non-tag files (global ID -1) are stored separately and never appear in tag search or browse results. Resources are non-tag files; they are listed under their owning tag (above).
- **Asset data is never read unless a call needs its contents.** Reading a tag decompresses only its header and tag data sections; its resource data section and resource file entries stay untouched (only `view_bitmap` / `export_bitmap` read them, §6.8).
- Class names come from the tags' own file names (`<tag path>.<class name>`, §5.1; every tag in the sample modules has one, and each class code always has the same name). Fallback if a class's tags have no suffix: the definition file name (§5.3), then the class code. A class's definition is found by code, then by class name (§5.3).
- **Header scan** (second stage, builds the reverse reference index and the string index): reads every tag copy's header and tag data sections (never resource data) and stores its dependency list, the global ID and class of every non-null tag reference field (from the tag data: the only exact source for references without a dependency entry, Q12), and its string table. Rebuilt per module when a module changes, like the index (decisions D50). Run on the first call that needs it (`find_references` with `in`/`both`, `search_values`, `tag_copies`), then cache it to disk next to the index. That first call MAY take a long time; its result says `(header scan built: N tags, T s)`.
- `reindex(what=all|index|header_scan|results, module?, class?, build?)`: forces a rebuild, for when the automatic checks aren't enough (e.g. a file was replaced with the same size and time).
  - `what`: `index` = module file lists (§6.2 first stage); `header_scan` = the header scan; `results` = the results cache (§6.9); `all` = everything.
  - `module`: only modules whose path contains this (case-insensitive). `class`: only tags of this class (for `header_scan` and `results`).
  - Returns what was rebuilt (modules, tags, cached results dropped) and how long it took. `header_scan` and `results` rebuild lazily: they're dropped now and rebuilt on the next call that needs them.
- Performance targets (measure on the real Forge install): index cold < 10 s, warm < 1 s; header scan < 2 min.

### 6.3 Search and browse (`FR-2`)

**Common filters.** Every list tool that takes one of these parameters gives it exactly this meaning, so the agent learns them once. All given filters must match (AND). They are implemented once and shared (`docs/decisions.md` D7).
- `query` + `mode=default|regex|exact`: name match. `default` = case-insensitive substring of the tag path (`search_tags` also applies Reclaimer's class rule, §5.5); `regex` = Rust `regex` syntax, case-insensitive, against the tag path (use `a|b` for OR); `exact` = tag path or display name equals the query, case-insensitive.
- `class`: class code or class name, exact, case-insensitive.
- `kind`: entry kind: a class code or class name (tags), a resource kind (`mesh resource`), or `file` (other files); exact, case-insensitive.
- `module`: case-insensitive substring of the module path.
- `min_size` / `max_size`: uncompressed size, inclusive, in bytes or with a `KB`/`MB`/`GB` suffix (decimal units).
- `offset` / `limit`: paging (§7.4).

- `search_tags(query, class?, module?, mode=default|regex|exact, build?, offset=0, limit=50)`
  - `default`: Reclaimer's rule (§5.5).
  - `regex`: Rust `regex` syntax, matched case-insensitively against the tag path.
  - `exact`: the tag path or display name equals the query (case-insensitive).
  - `class`: class code or class name (exact, case-insensitive). `module`: substring of the module path (case-insensitive).
  - One row per global ID. Columns: global ID, class code, display name, copy count. Sorted by tag path.
  - When the hits span more than one class, the summary gives the hits per class, most first (top 8, then `+N more`), so a broad search can be narrowed with `class=` in one step.
  - No hits: a `hint:` says why. If the query matches tags but `class`/`module` removed them all, it says how many the query alone matches; otherwise it lists up to 5 similar folder or file names from tag paths ("did you mean", edit distance, as in §6.4.2), or suggests a shorter query (D22).
  - Always returns the total count.
- `browse(path="", build?, offset=0, limit=100)`: treats tag paths as folders. `path` is a folder prefix (case-insensitive, `\` separated, no trailing `\` needed). Returns (1) the immediate subfolders, each with the number of tags under it recursively, and (2) the tags directly in that folder. Never the full tree.
- `list_classes(build?)`: class code, class name, tag count, whether a definition exists.
- `list_modules(build?, offset=0, limit=100)`: module path, version, tag count, has load manifest, **status**: `ok`, or `!!` when the module's structural checks (§9, M1) failed or it couldn't be indexed (`docs/decisions.md` D14).
- `list_entries(tag?, query?, mode=default|regex|exact, kind?, module?, min_size?, max_size?, sort=name|size, build?, offset=0, limit=100)`: file entries (tags, resources, other files) with their kind and size, from the index only (nothing is decompressed). Common filters combine; with none, every entry of the build (paged).
  - `tag`: that tag's entry and its resources (default copy, §6.4.6; `module` chooses another).
  - `query` matches a resource by its owner's tag path, so `query="magnum", kind="bitm"` and `query="magnum", sort=size` work as expected.
  - Columns: entry type (`tag` / `resource` / `file`), name (display name; for a resource: owner display name + `[index:kind]`), kind, uncompressed size, compressed size (or `(not stored in this module)`, D17), module. `sort=size` sorts largest first.
  - Always returns the total count and the total uncompressed size of all matches.
- `list_resource_kinds(build?)`: each resource kind with its entry count and total size. Answers "what kinds of assets exist, and how much of each?".
- Sizes: text output shows decimal units with one decimal (`6.1 MB`, `512 B`); JSON gives bytes.

### 6.4 Read tag (`FR-3`) — the core feature

#### 6.4.1 Signature

`read_tag(tag, path?, section?, depth=2, detail=normal, options=auto, show_hidden=false, confirm_large=false, module?, format=text, build?)`

- `path` and `section` are mutually exclusive. If neither is given, the whole tag is read (subject to `depth`).
- `module`: read the copy from this module (substring match; must match exactly one copy). Default copy: §6.4.6.
- `depth`: how many levels of struct fields, array fields and tag block fields are expanded below the requested node. The requested node's own direct fields are always shown. `depth=0`: direct fields only; nested structs/tag blocks appear as one-line stubs. `depth=2` (default): two nested levels. Stubs say how to continue (§7.4).
- `detail`, `options`, `show_hidden`, `format`: §7.
- **Size guard** (since M4, decisions D38): a read with neither `path` nor `section` whose text result would exceed `large_read_tokens` (§6.1; tokens estimated as characters ÷ 4) is not returned. Instead the result is the tag's `outline` at depth 1 (decisions D41) plus `hint: whole read ≈ N tokens (L lines); read a path= or section=, or repeat with confirm_large=true`. With `confirm_large=true` the whole read is returned, preceded by `warning: large result, ≈ N tokens (L lines)`. It is the agent's equivalent of a confirmation dialog: a large result takes a deliberate second call. The CLI applies the same guard.

#### 6.4.2 Field paths

- Field names separated by `.`. Names are display names (§5.3, markers removed), matched case-insensitively; spaces are part of names. Real names can contain `.` and `[…]` (`Res. Area`, `peg threshold [0,1]`; `docs/format-notes.md` "Names"), so each step is matched against the names at its level: the longest name the text starts with, followed by the end, `.` or an index (decisions D35).
- `[i]` selects entry `i` (0-based) of a tag block field; `[*]` selects every entry. Example: `barrels[0].firing noise`, `barrels[*].rounds per second`.
- Struct fields and array fields are stepped into by name, without an index.
- A tag block field reached **without** an index shows its entry count and labels, and expands **entry 0** if `depth` allows (output says `showing [0]`, and with more than one entry `→ path=<block>[*]` for the rest, decisions D48). A path that passes **through** a tag block without an index uses entry 0 too (`barrels.rounds per second` = `barrels[0].rounds per second`).
- If a name doesn't match: return an error listing up to 5 closest names at that level (edit distance).
- If several sibling fields have the same name: the first is `name`, later ones are `name#2`, `name#3` (in field order). Output always uses these unique names.

#### 6.4.3 Sections

- `section="<heading>"` selects a section by heading text (case-insensitive). It returns the section's heading and every field up to the next heading at the same or a higher level (§7.3). Headings are class sections, explanations and group headings (§5.3); a class section outranks the others. Structs and tag blocks inside a section belong to it (D35).
- If the heading occurs more than once, return the list of matching sections with their field paths instead of reading.

#### 6.4.4 Field type table and reading

- The code contains our own **field type table**: for each `_field_*` type, its size in bytes, its value kind, and its component count/names. Sizes come from AusarDocs (§5.2) and from observation; record the source of each in `docs/format-notes.md`.
- **Unknown field type:** stop reading the current struct (or tag block entry) at that field; report it (§8). Other structs and the rest of the tag are still read.
- Value rendering rules (both formats):
  - Enum: number + label of the option at that position.
  - Flags: raw value + labels of set bits; set bits with no option are listed as unnamed bits.
  - Tag reference: target display name + global ID + allowed classes (`validtags`). The target name comes from the tag's own string table (so it's shown even if the target isn't indexed).
  - Block index: number + target tag block field + that entry's label.
  - string_id: resolved string, or hash if unresolved.
  - Multi-component (points, vectors, bounds, Euler angles, quaternions, planes, colours): named components (`min/max`, `x/y/z`, `i/j/k/w`, `r/g/b`, `a/r/g/b`, ...).
  - Data reference: size in bytes. Pageable resource: the resource's kind and size, `(mesh resource, 6.1 MB, not loaded)` (§12 Q9, answered); `(none)` when the field has no resource struct; `(not stored in this module)` when the resource's data isn't in the module (D17). Bitmaps can be viewed (§6.8).
- **Tag block entry labels:** the value of the entry's field marked `^`; if none, the first string, string_id or tag reference field in the entry; if empty or none, the entry index. Text output leaves out a block's label list, and a block index's label, when no entry has a label of its own (since M4, decisions D40); JSON keeps every label.

#### 6.4.5 Definition checks

Two checks run for every struct field, tag block field and the main struct:

1. **Size check (always enforced):** the definition's computed size must equal the actual size. For a tag block field: target data block size = entry count × entry size. For the main struct: main data block size = summed size of the definition's top-level fields. On mismatch, **do not show values** from that struct; report `!! definition size mismatch (definition N bytes, tag M bytes)` (§8).
2. **GUID check:** compare the definition's `srcguid` with the tag struct entry's type GUID for the same field. Config `guid_check = "off" | "warn" | "enforce"`, **default `warn`**. Q3 is answered (`srcguid` is a Windows GUID, §12), but about 0.2% of real tag blocks carry a GUID found in no definition while their size fits, so `warn` stays the default (decisions D32). `warn` adds a warning and keeps the values; `enforce` treats a mismatch like a size mismatch.

- Class with no definition: return tag info (global ID, path, class, copies, dependency list) and `(no definition for class 'xxxx')`.

#### 6.4.6 Which copy is read

- **Copy identity** is decided by the stored data, not by `assetChecksum` (§12, Q10): v27 = all copies have the same compression block checksums (hashes of the uncompressed data, already in the block list; no decompression); v23 (no block checksums) = identical compressed bytes, otherwise decompress both copies' header and tag data and compare. Results are cached (§6.9).
- If all copies are identical: read any one; output says `copies: N (identical)`.
- If copies differ: read the copy in the first module by module path sorted ascending (ordinal, case-insensitive), and output says `copies: N (differ; reading <module>; use module= to choose)`. This is a provisional rule (§12, Q5).

### 6.5 Definitions and outline (`FR-4`)

- `get_definition(class, path?, section?, depth=2, detail=normal, options=auto, show_hidden=false, confirm_large=false, format=text, build?)`: the schema from the definition only: fields, types, units, ranges, help, options, nesting, computed offsets (help and offsets at `detail=full`, decisions D37). No tag data. Whole reads have the same size guard as `read_tag` (§6.4.1), with the class's outline instead. Paths use §6.4.2 syntax; tag block fields are stepped into without an index (`[i]`/`[*]` are accepted and ignored). This is the **field dictionary**: definitions are not repeated inside `read_tag` output beyond what `detail` asks for.
- `search_definitions(query, class?, scope=all|names|options|help, build?, offset=0, limit=50)`: case-insensitive substring search across **all** definitions: field display names (`names`), enum/flag option names (`options`), help text, units and explanation text (`help`). Hits where the query is a whole word of the matched text come first, then the other substring hits (since M4; otherwise `age` lists every "damage" and "percentage" first). Each hit: class code + name, field path (tag block fields written `name[*]`), field type, and which part matched. Answers "which class controls X, and where?".
- `outline(tag?, class?, depth=2, build?)`: exactly one of `tag` or `class` MUST be given. Returns only the section tree: class sections, explanation headings, struct fields, array fields and tag block fields, each with its field path and number of direct fields; for a tag, also entry counts and labels for tag block fields. No values. **This is the first call for any large tag** (§5.3 list).
- `definition_status(build?)`: per class in the index: has definition (yes/no), and any size or GUID mismatches **detected so far** (kept in the results cache across runs, §6.9), with how many of the class's tags have been checked. It is also the **definition gap list** (decisions D33), the input for writing or dumping missing definitions (§10):
  - classes with no definition, with their tag counts;
  - size mismatches, each with the field path where it starts;
  - struct GUIDs found in no definition, each with its class and field path;
  - each class's main struct GUID (one per class, a layout fingerprint, `docs/format-notes.md` "GUIDs"), so a class whose layout differs between builds shows up.
  The list can be exported (JSON) for that work.

### 6.6 Analysis (`FR-5`)

- ~~`get_field(tag, path)`~~ dropped (2026-11-02, decisions D53): it is `read_tag(tag, path=…, depth=0)`; `read_tag`'s description says so, which costs less than one more tool in every session's tool list.
- `find_references(tag, direction=out, recursive=false, max_depth=3, class?, build?, offset=0, limit=100)`
  - `class`: return only referenced/referencing tags of that class. With `recursive=true` the walk still follows references of every class, but the result is a flat list of the matching tags, each with its depth and the tag that references it (`via`). Example: every bitmap a weapon uses, through its projectiles and effects.
  - `out`: tags this tag references, from the tag header's dependency list and tag reference list (works without a definition). If a definition exists, each reference also gets its field path (from the reference's (data block, offset)). Null references (field global ID -1) are omitted. References without a dependency entry are included (§5.2).
  - `in`: tags that reference this tag (uses the header scan, §6.2). The dependency list alone misses references without a dependency entry (§5.2, Q12).
  - `both`: both lists, labelled.
  - `recursive=true` (only with `out`): follow references up to `max_depth` levels, as a tree; a tag already shown earlier in the tree is printed once more as `(see above)` and not expanded again.
  - Results grouped by class, with counts per class.
- `query_field(class, path, filter?, sort=none, build?, offset=0, limit=100)`: the same field across every tag of a class. `path` MAY contain `[*]` (one row per entry). Columns: display name, (entry label), value.
  - `filter` grammar: `<op> <value>`, op ∈ `== != > >= < <= contains`. Numbers compare numerically; enums compare by label (or by number if the value is a number); flags support `contains <option label>`; strings, string_ids and tag references support `==`, `!=`, `contains` (case-insensitive). Filters on multi-component values are an error.
  - `sort`: `none`, `asc`, `desc` (by value; same type rules as filter).
  - Tags that fail to read are listed under `skipped` with the reason; they never stop the query.
- `diff_tags(a, b, path?, detail=brief, build?)`: both tags MUST have the same class (otherwise error). Lists only fields whose values differ, as `path: a-value → b-value`. Tag block fields: compare entry counts, then entries by index; extra entries are listed as `only in a` / `only in b`. `a` and `b` MAY be in different builds (`xbox:...` vs `forge:...`).
- `search_values(term, class?, build?, offset=0, limit=50)`: tags whose string table contains `term` (case-insensitive substring), i.e. string_ids and referenced tag names. Uses the header scan. Each hit lists the matching strings.
- `tag_copies(tag, build?)`: every copy: module, asset checksum, sizes; and whether all copies are identical (§6.4.6 copy identity).
- `map_tags(module, class?, build?, offset=0, limit=100)`: tags listed in that module's `loadmanifest`, grouped by class. Error if the module has no manifest.
- `which_maps(tag, build?)`: modules whose `loadmanifest` includes the tag.
- `compare_builds(build_a, build_b, class?, offset=0, limit=100)`: by global ID: tags only in A, only in B, and in both but different (stored data differs, §6.4.6 copy identity; across v27/v23 builds, by comparing decompressed header and tag data). Counts first, then rows.

### 6.7 Developer tools (CLI only, not exposed over MCP)

- `raw-view <tag> --data-block N --offset X --length L`: hex dump with the definition's field layout overlaid.
- `header-dump <tag> [--module M] [--build B]`: all tag header sections, as stored: one titled table per list (`## header`, `## dependencies`, `## data blocks`, `## tag structs`, `## data references`, `## tag references`, `## string IDs`, `## zonesets`, `## zoneset resources`, `## zoneset tags`), in the `table`, `text` and `json` formats. Dependencies and tag references show global IDs and display names; tag references also show what their field holds (§5.2). Zoneset names are resolved from the tag's string_ids, and zoneset resources are named from the index. Fields of unknown meaning are shown by their offset (`unknown4`). When the class has a definition, each tag reference also shows its **field path** (e.g. `barrels[0].projectile`, `attachments[1].type`), the same path `find_references` gives; the raw (block, offset) columns stay (since M3). Decisions D28, D29.
- `preferences [set <key> <value> | unset <key>]`: show or change the display preferences (§6.1). Needs no config and loads no game files.

### 6.8 Bitmaps (`FR-6`, untiled only in v1)

- Applies to `bitm` tags. Reads each bitmap's resource using `(0008)bitmap_data_resource_definition.xml` and the bitmap tag's own fields (format, size, mip count, type).
- Textures with a non-zero `tileMode` (Xbox One tiled layout): MUST NOT be decoded in v1. Return `!! texture is tiled (tileMode N); de-tiling is not supported yet`.
- `view_bitmap(tag, image=0, mip=auto, max_size=1024, build?, module?)` — for agents. `image` is the index into the bitmap tag's list of images (a `bitm` tag can hold several). `mip=auto` picks the largest mip whose width and height are both ≤ `max_size`. Returns **one MCP image content item (PNG)** plus **one text content item**:
  - **Preview sheet PNG:** colour image (alpha composited over a grey checkerboard), then R, G, B and A each as a separate greyscale panel, side by side, each labelled. Cubemaps: the six faces in a cross layout. Arrays and 3D textures: a grid of slices, labelled by index. HDR/float formats: tone-mapped (Reinhard) for display. Total sheet width ≤ 2048 px (scale panels down if needed).
  - **Text summary:** format (DXGI or Xbox name, plus the DXT name for BC1–3), width × height, mip count and the mip shown, image count, type (2D/cube/array/3D), colour space, tile mode, per-channel min/max/mean (computed on the shown mip, before tone-mapping), and notes: `alpha unused (all 255)`, `single channel`, `looks like a normal map (B mean > 0.9)`.
  - The CLI writes the sheet to a file and prints the summary.
- `export_bitmap(tag, format=dds|png|tga|tif, channels=combined|split, image=all, out_dir, build?, module?)` — for the user. `dds` writes the original compressed data with all mips (lossless; DX10 header when needed). `png`/`tga`/`tif` write the decoded top mip. `split` writes `<name>_red`, `_green`, `_blue`, `_alpha` greyscale files (not applicable to `dds`; error). Returns the written file paths.
- Required decoding: BC1–BC7 and the uncompressed formats used by Forge bitmaps (list them from real data during M7).

### 6.9 Results cache (`FR-7`)
- Expensive results that depend only on the game files and definitions are stored on disk in `cache_dir`, next to the index, and reused across calls, processes and sessions. Work done for one call (by any agent or the CLI) speeds up every later call that needs it:
  - copy identity per tag (§6.4.6);
  - the header scan (§6.2);
  - definition check results per class: size and GUID mismatches, and how many tags were checked (§6.4.5, `definition_status`).
- Each cached result is keyed by what it depends on: the module files (path, size, modification time) and, where relevant, the definition file. It is dropped when any of them changes. `reindex(what=results)` clears it, optionally per module or class.
- The cache never changes a result, only how fast it comes (§7.6, no hidden state). The one exception is `definition_status`, which reports what has been checked so far and says how much that covers (`checked 120 of 340 tags`).

## 7. Output format

### 7.1 Formats

- Every tool takes `format=text|json`. **MCP default: `text`.** The CLI also has `table`, for people: the `text` content with space-padded columns, numbers right-aligned and the long name/path column last. **CLI default: `table` when stdout is a terminal, `json` when it is piped or redirected** (decisions D23). `table` is never offered over MCP. In a terminal, `table` is colored: the part of each name that matched the query is highlighted, headers are bold, `!!` red, hints cyan, the paging line dim. Never in `text`, JSON, piped output or over MCP (D24).
- The internal model is one tree of nodes (§7.7). `text` and `json` are two renderings of it. They MUST carry the same information at the same `detail` level.
- A tool result MUST NOT contain the same content twice (e.g. text plus an identical structured copy).

### 7.2 Text format: field lines

One field per line. In `text` (what the AI reads), columns are separated by exactly two spaces; in `table` (people, CLI), they are aligned within each run of fields under one heading (decisions D36):

```
<indent><name>  <short type>  <value>  <units>  <extras>
```

- Indent: 2 spaces per nesting level: the main struct's fields have none, each struct, array or tag block entry adds 2 (headings too) (D36).
- Short type names are defined in `docs/AGENT_GUIDE.md` §4 (normative). Examples: `real`, `int16`, `enum16`, `flags32`, `bounds`, `tagref`, `stringid`, `blkidx`, `block[N]`, `struct`, `data`, `function`, `resource`.
- Units and range (`[min-max]`) follow the value.
- **Angle types** (`_field_angle`, `_field_angle_bounds`, `_field_real_euler_angles_2d`, `_field_real_euler_angles_3d`) store radians, though definitions label them in degrees (`docs/format-notes.md` "Angle units"): the exact stored value is marked `rad`, and the units show the value converted to degrees (3 decimals) before the definition's unit text; JSON adds `degrees` (since M4, decisions D44).
- Extras: options (§7.5), markers (`detail=full`), states (§8).
- Mapping-function structs render as one line: `<name>  function  (curve, N bytes, #xxxxxxxx)`: the size and a fingerprint of the curve's bytes (MurmurHash3 x86 32, seed 0, as 8 hex digits; JSON `fingerprint`), so equal curves can be recognised without decoding them (since M4, decisions D46).
- The internal struct and `vtable space` are never shown, even with `show_hidden`.

### 7.3 Text format: headings and sections

- Line 1 of every `read_tag` / `get_definition` / `outline` result is the **tag line** (or definition line), starting with `# `:
  `# <class code> <display name>  <global ID>  build=<b>  copies: ...  resources: N (<total size>)` (definition line: `# <class code> <class name> (definition)`). `resources:` is omitted when the copy has none. At `detail=full`, one line per resource follows the tag line: `  resource [i]  <kind>  <size>`.
- `## <LABEL>`: a **class section** (top-level section label, §5.3). Only these use `##`.
- `### ` and deeper: explanations, struct fields, array fields and tag block fields. Level = 3 + nesting depth (a struct inside a struct gets `####`, and so on).
  - Explanation: `### <heading>`, then the first line of its text as `> <text>`, with ` […]` appended if cut. Full text with `detail=full`. Explanation with an empty heading: only the `>` line (at the current indent). Explanation with no text: only the heading.
  - Struct field: `### <name>  struct`.
  - Array field: `### <name>  array`.
  - Tag block field: `### <name>  block[N] {label0, label1, ...}  showing [i] <label>`. Labels list: at most 8, then `+N more`.
- A section contains the fields after its heading up to the next heading at the same or a higher level.
- When `read_tag` targets a `path` or `section`, the first heading after the tag line states it: `### barrels[1]  (path)` or `### EXPOSURE  (section)`.

Example (`read_tag`, `detail=normal`; values illustrative):

```
# weap objects\weapons\rifle\assault_rifle\assault_rifle.weapon  0x00A1B2C3  build=forge  copies: 3 (identical)
## OBJECT
### physics  struct  → path=physics
## WEAPON
### barrels  block[2] {primary, secondary}  showing [0] primary
> Controls how each barrel fires. […]
  rounds per second   bounds    8.5..8.5
  trigger mode        enum16    2 charge  {normal|latch|charge|latch-autofire}
  flags               flags32   0x00000005 {tracks fired projectile, prevents grenade throwing}  (+14 options)
  projectile          tagref    objects\weapons\rifle\assault_rifle\projectiles\bullet.projectile  0x00D4E5F6 [proj]
  parent barrel       blkidx    1 → barrels[1] secondary
  default function    function  (curve, 24 bytes)
  #### shots  block[3] {first, second, third}  → path=barrels[0].shots
```

### 7.4 Text format: lists, paging and stubs

- List results (`search_tags`, `browse`, `list_*`, `query_field`, `find_references`, `search_values`, `compare_builds`, ...) are tab-separated tables: one header line, then one row per item.
- When many rows share a path prefix, print `in <prefix>:` once and show the rest relative to it.
- Every paged result ends with `… <remaining> more (offset=<next offset>)`.
- Every node not expanded because of `depth` ends its line with `→ path=<field path>`.
- `brief` level (§7.6): no headings except the tag line; each line is `<full field path>  <value>` (e.g. `barrels[0].rounds per second  8.5..8.5`).

### 7.5 Options

`options=auto|all|none`:

- `auto`: enums with ≤ 12 visible options list all of them as `{a|b|c}`; larger enums show only the chosen label plus `(+N options)`. Flags show the set bits as `{a, b}` plus `(+N options)` for the rest.
- `all`: list every visible option. `none`: no options.
- Hidden options (§5.3 provisional `!` rule) are excluded from lists and counts unless `show_hidden=true`. **A set flag bit or the chosen enum value is always shown, even if its option is hidden.**
- When one field appears in many tag block entries (`[*]`), its options are printed once in a `legend:` block at the end, not on every line.

### 7.6 Detail levels and hidden fields

| `detail`           | Shows                                                                                                                                                    |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `brief`            | Field paths and values only (enum labels, tag reference display names, resolved string_ids, state markers). No headings, units, options or explanations. Lines of one tag block entry are grouped under an `in <entry path>:` line, indented, with relative paths (since M4, decisions D40). A tag block, and a struct or block with a warning or error, keeps a line of its own with its entry count and the warning or error; one not expanded ends in `→ path=…` (decisions D45). |
| `normal` (default) | + headings and section structure, first line of explanations, units, ranges, options per `options`.                                                      |
| `full`             | + help text, full explanations, field offsets (hex, relative to the start of their struct), markers, `(not read)` lines after a read failure.            |

- **Hidden fields** (shown only with `show_hidden=true`): `_field_pad`, `_field_skip`, `_field_tag`, and any field whose markers include `!` (provisional, §5.3). Shown hidden fields are marked `(hidden)`.
- Data reference fields and pageable resource fields are **not** hidden; they show their size or resource state.
- No hidden per-session state: the same call always returns the same output.

### 7.7 JSON format

- Same tree as text. Node keys: `name`, `path`, `type` (XML type name, e.g. `_field_short_enum`), `short_type`, `value`, `label`, `raw`, `set`, `unnamed_bits`, `options`, `units`, `range`, `help`, `markers`, `offset`, `count`, `labels`, `index`, `children`, `target` (tag reference: `{ "name", "id", "valid_classes" }`), `state`, `reason`, `warning`. Omit keys with no value.
- Example nodes:

```json
{ "name": "trigger mode", "path": "barrels[0].trigger mode", "type": "_field_short_enum",
  "short_type": "enum16", "value": 2, "label": "charge",
  "options": ["normal", "latch", "charge", "latch-autofire"] }

{ "name": "flags", "path": "barrels[0].flags", "type": "_field_long_flags", "short_type": "flags32",
  "raw": 5, "set": ["tracks fired projectile", "prevents grenade throwing"], "options": ["..."] }

{ "name": "barrels", "path": "barrels", "type": "_field_block_64", "short_type": "block",
  "count": 2, "labels": ["primary", "secondary"], "index": 0, "children": [ ] }

{ "name": "projectile", "path": "barrels[0].projectile", "type": "_field_tag_reference_64",
  "short_type": "tagref",
  "target": { "name": "objects\\...\\bullet.projectile", "id": "0x00D4E5F6", "valid_classes": ["proj"] } }

{ "name": "exposure", "path": "exposure.exposure", "type": "_field_real", "short_type": "real",
  "value": 1.5, "units": "stops", "help": "the target exposure (ONLY USED WHEN AUTO-EXPOSURE IS OFF)" }
```

## 8. Empty, missing and unreadable values

Every output MUST say **why** something has no value. Three categories, which MUST look different:

1. **Empty in the tag** (a real fact about the tag): `(empty)`, `(none)`, `(none set)`.
2. **Not described by the definition** (the value is real, its meaning unknown): `(no definition…)`, `(no options defined)`, `(unnamed @0x1C)`, `(no option N…)`.
3. **The tool could not read it** (our problem, not a fact about the tag): lines starting with `!!`, and `(not read)`.

A read failure in one struct MUST NOT stop the rest of the tag from being read (unlike Reclaimer, §5.5).

| Situation                                        | Text                                                                      | JSON `state`          |
| ------------------------------------------------ | ------------------------------------------------------------------------- | --------------------- |
| Tag block field with 0 entries                   | `barrels  block[0] (empty)`                                               | `empty`               |
| Null tag reference                               | `projectile  tagref  (none) [proj]`                                       | `none`                |
| Tag reference to a tag not in this build's index | `... 0x00D4E5F6 [proj] (not in indexed modules)` (name still shown)       | `not_found`           |
| Null data reference / 0 bytes                    | `data  (empty)`                                                           | `empty`               |
| Data reference with data                         | `data  (N bytes)`                                                         | —                     |
| Pageable resource                                | `(resource, not loaded)`, or `(<kind>, <size>, not loaded)` once Q9 is verified / `(none)` | `not_loaded` / `none` |
| Null struct (tag struct entry target -1)         | `name  struct  (none)`                                                    | `none`                |
| Empty string                                     | `""`                                                                      | —                     |
| string_id = 0                                    | `(none)`                                                                  | `none`                |
| string_id not in the tag's string table          | `0x9B555AD2 (unresolved)`                                                 | `unresolved`          |
| Flags with no bits set                           | `0x00000000 (none set)  (+N options)`                                     | `none_set`            |
| Set bit with no option                           | `unnamed bits: 9, 12`                                                     | `unnamed_bits` key    |
| Enum value with no option at that position       | `7 (no option 7; 4 defined)`                                              | `no_option`           |
| Enum/flags field with no `<item>`s               | `3 (no options defined)`                                                  | `no_options`          |
| Block index -1                                   | `-1 (none)`                                                               | `none`                |
| Block index ≥ entry count                        | `5 (out of range; block has 2)`                                           | `out_of_range`        |
| Field with empty name in the definition          | `(unnamed @0x1C)`                                                         | `unnamed`             |
| Section whose fields are all hidden              | `### HEADING  (N hidden fields)`                                          | `hidden_only`         |
| Class with no definition                         | tag info, then `(no definition for class 'xxxx')`                         | `no_definition`       |
| Entry described but not stored in its module (D17) | `(not stored in this module)`                                           | `stored: false`       |
| Module file incomplete / damaged / unreadable (D14) | `!! incomplete file: …` / `!! damaged: …` / `!! not indexed: …` (`list_modules` status) | `status` |
| No tags of a class in the indexed modules (D18)  | empty result + `hint:` (says whether the class has a definition)          | `hints`               |
| Unknown field type                               | `!! read stopped at <field path>: unknown field type _field_xyz`          | `error` + `reason`    |
| Size mismatch (§6.4.5)                           | `!! definition size mismatch at <path> (definition N bytes, tag M bytes)` | `error`               |
| GUID mismatch, `warn` mode                       | value shown + `warning: definition GUID mismatch`                         | `warning` key         |
| GUID mismatch, `enforce` mode                    | `!! definition GUID mismatch at <path>`                                   | `error`               |
| Any other read error                             | `!! read failed at <path>: <reason>`                                      | `error`               |
| Fields after a failure in the same struct        | omitted; at `detail=full` listed as `(not read)`                          | `not_read`            |

`brief` uses the same markers.

**Tool-level errors** (bad arguments, unknown tag, file I/O): text `!! error: <message>` + optional `hint: <next step>`; JSON `{ "error": "...", "hint": "..." }`. The MCP server MUST return these as tool results, never crash. Batch tools (`query_field`, header scan) list per-tag failures under `skipped` and continue.

## 9. Milestones and acceptance criteria

The config format with `[[build]]` (§6.1) is used from M1; only one build is needed until M6.

| #   | Scope                                                                                                       | Done when                                                                                                                                                                                                                               |
| --- | ----------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| M1  | Module reader (v27 and v23), index (§6.2 first stage), `search_tags`, `browse`, `list_classes`, `list_modules`, `list_entries`, `list_resource_kinds` in the CLI | Every module in the deploy folder indexes without errors; per module, these cross-checks pass: the data area ends exactly at the end of the file, `firstResourceIndex` = `numFiles` − `numResources`, every list and every file's data lies inside the module, each file's header + tag data + resource sizes = its total uncompressed size, each file's three section block counts add up to its block count, and no global ID appears twice; every compression block of every file entry (tags, resources and other files) decompresses to its stated size (integration test, not done at runtime); the index lists each tag's resources (count, kind, sizes); 5 sample searches return what the user expects; CI builds and unit tests pass on Windows, Linux and macOS (§13) |
| M2  | Tag decompression, tag header parsing (all sections), `header-dump`                                         | For **every** tag of every sample module (both builds; decided 2026-11-01, was "10 tags across ≥ 5 classes"): section sizes add up to the header's sizes; dependencies print as readable display names                                                                                                                |
| M3  | `read_tag`, `get_definition`, `outline`, `search_definitions`, definition checks, §7 text/JSON, §8 states   | Verification checks 1–7 (below) pass for every tag of `weap`, `proj`, `bipd`, `vehi`, `effe`, with zero unexplained failures; the user's spot values (check 9) match                                                                    |
| M4  | MCP server for everything above + agent instructions (§9a); in-memory caches (§13); the size guard (§6.4.1); fixes from the simulated session (decisions D38) | A fresh Claude Code session with only the server instructions and tools answers the M4 example questions listed below this table without reading a whole large tag, and without stating values from `!!` output as facts, with the tokens per question recorded in `docs/progress.md`; the guide-coverage test (§9a) passes                   |
| M5a | Token budget test (first step, below), header scan (§6.2, with Q12), `find_references`, `tag_copies`, `search_values` (§6.6) | The token budget test passes; the header scan builds on both samples (time recorded; target < 2 min on a full install); `find_references(in)` matches a manual check on 3 tags; a fresh session answers "Which tags reference this projectile?" within its budget |
| M5b | `query_field`, `diff_tags` (same build), `map_tags`, `which_maps` (with Q5, Q8), `definition_status`, results cache (§6.9) | `query_field` over all `weap` tags < 5 s; a fresh session answers the other M5 example questions, each within its token budget |
| M6  | Second build (Xbox One Guardians), `compare_builds`, cross-build `diff_tags`                                | Both builds indexed together; campaign scenario tags appear as Xbox-only; `definition_status` reports per class whether the definitions fit the Xbox build                                                                              |
| M7  | §6.8 bitmaps (untiled)                                                                                      | For 10 Forge bitmaps (diffuse, normal map, cubemap, HUD element, alpha mask): two independent decoders agree; the user confirms the preview sheets look right; exported DDS files re-read by our own DDS reader give identical data     |

**Verification without Reclaimer.** The user will not install Reclaimer; no test or acceptance check may need it. Correctness comes from these independent checks, all automated:

1. **Header ↔ definition cross-check (the main check).** The tag header lists, independently of any XML, where every tag reference field, tag block field, struct and data reference field sits (§5.2: tag reference list, tag struct list, data reference list, each with data block + offset). Walking the tag with the definition MUST find exactly the same set: every `tagref` field the definition places at an offset MUST appear in the tag reference list at that (data block, offset), and every list entry MUST be reached by some definition field. Same for tag block fields vs tag struct entries, and data reference fields vs the data reference list. A missing or extra entry means the definition (or our reading) is misaligned at that point. Report the first divergence with its field path.
2. **Reference values agree:** each tag reference field states its listed name's length; a field with a dependency entry holds that entry's global ID and class (checked since M2, `TagHeader::check_tag_data`). A field without one may hold any global ID (§5.2).
3. **GUIDs agree:** definition `srcguid` = tag struct entry GUID (§6.4.5, once Q3 is answered).
4. **Sizes agree:** tag header section sizes add up to the file entry's sizes; definition sizes match data block sizes; tag block entry counts × entry size = target data block size.
5. **Hashes agree:** every resolved string_id hashes back to its stored value; MurmurHash3 matches known values (`"__default__"` → `0x9B555AD2`).
6. **Plausibility:** enum values within their option count, block indices within range, entry counts ≤ the definition's `max`, floats finite. Report the rate per class; isolated violations are fine (data can be odd), widespread ones point to misalignment.
7. **Whole-build sweep:** read every tag of every class with a definition; the run reports `!!` errors and check 1–6 failures per class. Target: zero unexplained failures for the M3 classes; others listed in `definition_status`.
8. **Bitmaps:** decode each test texture with two independent BCn decoders and compare; the agent views the preview sheets (§6.8) and the user confirms a few look right.
9. **The user's game knowledge:** a few spot values the user knows from playing (e.g. a weapon's magazine size) as named integration tests.

Example questions for M4 (answerable with the M1–M3 tools):

- "Find every tag related to the assault rifle."
- "What's in `objects\weapons\rifle\`?"
- "Show the firing settings of the battle rifle's first barrel, and every possible trigger mode."
- "What's different between the assault rifle's and the SMG's barrels?" (two path reads; `diff_tags` makes it one call in M5)

Example questions for M5 (they need the §6.6 tools; the first is M5b's, the second M5a's, the last two M5b's, since they need tools from both halves):

- "List the magazine size of every weapon."
- "Which tags reference this projectile?"
- "What would it take to make the battle rifle use a battery (a percentage, like the plasma pistol) instead of bullets, and what else would that change (HUD, ammo pickups)?" (decisions D38: simulated before M4 at about 12,000 tokens over 41 calls, `docs/progress.md`)
- "Would that battery level be kept if the weapon is dropped and picked up again?" The answer may depend on engine behaviour no tag stores: the agent shows the related fields and examples from other weapons, and says what is inference (AGENT_GUIDE "What a change affects").

**Token budget test (first step of M5a).** An automated, ignored real-file test replays the call sequences of the M4 acceptance session and its retests (and later the M5 ones) and fails when a call's text result grows past its budget: the measured size plus 20%. It keeps later output changes from making answers more expensive. It comes after M4 because the sequences must be real ones, recorded from a fresh session, not guessed.

**Testing:** unit tests for the field type table (every type used in the definitions), name parsing (§5.3, including `#`/`:`/range/markers), field path parsing, MurmurHash3 (known values), enum/flag decoding, custom-label classification. Snapshot tests of `read_tag` text and JSON on fixtures. An integration test suite, ignored by default, runs against a real deploy folder given by an environment variable. No game files are committed; fixtures are synthetic or tiny extracted samples.

### 9a. Agent instructions (part of M4)

`docs/AGENT_GUIDE.md` is **normative**: output format, short type names and markers MUST match it. Any change to output updates the guide in the same commit.

**Tag internals are explained wherever the AI meets them** (decisions D30). Whenever a tool's output shows anything that comes from a tag's header or sections, the guide explains in plain words what that list is and what it tells about the tag, in the same commit as the output. This covers dependencies, tag references and their targets, null references, references without a dependency entry (§5.2), string IDs and the tag's string table, data blocks, resources, and header ↔ definition mismatches. The AI must never have to guess what such an item means.

Delivered in three layers:

1. **MCP server `instructions`** (sent once per connection; keep to about 20 lines). Exact text:

```
h5tags reads Halo 5 .module tags (builds: see list_modules / the build parameter). Values come only from these tools; never fill them in from memory.
Workflow: find (search_tags / browse / search_definitions) → outline(tag) → read_tag(section= or path=) → find_references → compare (query_field / diff_tags, detail=brief).
Never read a whole large tag (scnr, effe, vehi, bipd, char, gint, unit, weap): call outline first.
detail: brief for many tags, normal by default, full only for fields you explain or change.
Text output: '# ' tag line, '## ' class section, '### '+ sections/structs/blocks, indented lines = fields (name, type, value, units, extras).
'→ path=...' = not expanded; call again with that path. '… N more (offset=X)' = next page.
(empty) (none) (none set) = real facts about the tag. (no definition…) (no option…) (unnamed…) = value is real, meaning unknown.
(unresolved) = string_id not resolved; don't guess it. (not in indexed modules) = say so; don't assume it's missing from the game.
'!!' or (not read) = the TOOL failed: never state those values, or their absence, as facts; tell the user. 'warning: definition GUID mismatch' = the values were read; use them and mention the warning.
copies: N (differ) = the tag differs between modules; say which module's copy you used.
Field names and help text are hints about behaviour, not proof; say where an explanation comes from.
Full guide: resource h5tags://guide.
```

2. **MCP resource `h5tags://guide`**: serves `docs/AGENT_GUIDE.md`.
3. **Tool descriptions**: one or two sentences each: what it does, when to use it, and the usual next call. Parameter descriptions list valid values (e.g. `detail: brief|normal|full`).

**Self-explanatory results (MUST).** The agent must be able to understand everything a tool returns:
- Every column, marker, state, summary line and status a tool can output is explained in `docs/AGENT_GUIDE.md` (and briefly in the tool's description when it's specific to that tool), including what the agent may and may not conclude from it.
- Renderers take their markers and column names from one central list. A unit test checks that every entry in that list appears in `docs/AGENT_GUIDE.md`, and fails if one is missing. A new marker can't ship undocumented.
- Rare or partial results explain themselves with one `hint:` line in the result (e.g. `hint: only 120 of 340 tags checked so far; run query_field or read_tag to check more`). Common output carries no extra text, to save context.

## 10. Backlog after v1 (priority order)

Each item reuses the v1 tools; none changes v1 behaviour.

1. `read_data(tag, path, as=auto|text|hex, offset, length)`: show a data reference's bytes. `auto` = text if ≥ 95% printable, otherwise a hex preview and size.
2. Scripts: `read_script(tag)` for `luas` (lua*script, field `luaFileData`) and `hsc*`(lua, field`source`): source text if stored as text, otherwise `compiled bytecode (N bytes)`with its header bytes.`find_scripts(term)`.
3. In-game text: `lookup_text(string_id | term, lang=english)` using `unic` (multilingual_unicode_string_list) tags; then show display text next to string_ids where available.
4. Forge mode palette: `forge_palette(path?)`: the Forge object menu as a tree, built from `fpal` (ForgePaletteDefinition) tags via their `Parent Palette` references, with display names; plus `forg` (forge_globals).
5. `entry_points()`: global tags to start from: `matg`, `mulg`, `forg`, `gegl`, `wezr`, `wgbl`, `lgtd`, `gmeg`, `rqgs`, `ffgt`, `vtgl`, `glda`, `glpa`, and each map's `scnr`.
6. `map_contents(scenario, class?)`: a `scnr` tag's object palettes and placement counts.
7. Field notes: `note(class, path, text)` / `notes(class?)` stored in `notes_file` (TOML, one entry per class + field path). Shown at `detail=full` as `note (user): ...`. Explicit and user-editable. Later extension (agreed with the user, not scheduled): the agent may also write notes for future sessions, marked `note (agent, unverified)`, each with its evidence (tag, field, value); the guide tells agents to treat them as hints, never as values.
8. Xbox One texture de-tiling: load the user's `xg.dll` (with `kernelx.dll` next to it) via `libloading`, configured per build as `xg_dll = '...'`; Windows x64 only; behind a cargo feature; never shipped. Use the library's 2D texture address computer to map tiled → linear per mip. Implement from the library's behaviour, not from Reclaimer's code. Later alternative: a pure-Rust de-tiler based on AMD's open-source address library (research).
9. Mapping functions: decode the `function` struct's data into readable curves (research; not documented in AusarDocs).
10. Definition dumping (research): produce definitions for the classes and struct types in the definition gap list (`definition_status`, §6.5): missing classes, layouts that changed between builds, struct GUIDs in no definition. Field names and layouts are not in the module files, so this needs another source; Reclaimer's code stays off-limits (§3). Whatever it produces is checked with the M3 survey and checks (§9) before use.

## 11. Future (v2+)

- Editing: `set_field(tag, path, value, dry_run=true)`. Writes to a copy of the module, keeps a backup, recompresses, recomputes checksums (each compression block's checksum: the first 64-bit half (h1) of Murmur3 x64 128, seed 0, of the uncompressed data (verified on every Forge sample block); module header checksum: Murmur3 x64 128 over the header with the checksum field zeroed + file list + resource list + compression block list), and verifies by re-reading. Start with fixed-size values (numbers, enums, flags); size-changing edits (adding entries) come later.
- Reading other resource types (`tag_resources`).
- Halo Infinite.
- Zoneset info: what the game does with zonesets (their layout is known since M2: names, tags and the resources each loads; §5.2).
- Extracting raw tag or resource data to files.

## 12. Open questions

Resolve each during the milestone noted; record the answer in `docs/format-notes.md` and update this PRD.

**Research method (decisions D31):** investigate an unknown with a **survey test** first. That is one ignored real-file test that, in a single run over every tag, reports for each field: its distinct values (overall and per class), whether it is constant per class, how it relates to the other fields (equal, a count or offset of something), and whether it equals a hash of something known (tag path, class, sizes, the `assetChecksum` halves). Build it the first time a milestone needs an unknown field, then reuse it. One run replaces many guess-edit-run rounds.

| #   | Question                                                                                                                                             | Needed by |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | --------- |
| Q1  | ~~Module and tag header versions of the Xbox One Guardians files~~: **answered**. Modules: version 23. Tag headers: version 27, the same as Forge (every sample tag checked; `docs/format-notes.md`). | done      |
| Q2  | Meaning of the `* ! ~ &` name markers. Until answered, `!` = hidden (§5.3).                                                                          | M3        |
| Q3  | ~~Do definition `srcguid` values equal tag struct entry GUIDs, and in what byte order?~~ **Answered (M3 survey, `docs/format-notes.md` "GUIDs"):** yes, as Windows GUIDs (first three groups little-endian), never in the as-written order. Custom structs always match; tag blocks in 99.8% (the rest have GUIDs in no definition); main struct GUIDs are one per class and in no definition. | done      |
| Q4  | Are Forge PC textures ever tiled (non-zero `tileMode`)?                                                                                              | M7        |
| Q5  | Which copy of a tag does the game use when copies differ? Until answered, §6.4.6 rule. Lead: a map's `.mapinfo` file lists its module stack in order (base `-rtx-1`, then `-rtx-11587-1`, `-rtx-12719-1`); later layers probably win (unverified). | M5b       |
| Q6  | How a resource identifies its type (to pick a `tag_resources` layout). Lead: resource file names end in `[index:kind]` (verified on v23); check whether `kind` maps to a `tag_resources` layout. | after v1  |
| Q7  | ~~Array fields have no count in the definitions: are all arrays single-instance, or is the count stored elsewhere?~~ **Answered (M3 survey):** single-instance; with that rule every Forge main struct (62,976) and every checked tag block (790,750) has the definition's exact size. | done      |
| Q8  | Do any tags need data merged from several copies (rather than picking one)?                                                                          | M5b       |
| Q9  | ~~How does a `_field_pageable_resource_64` field link to its resource file entry?~~ **Answered (M3, `docs/format-notes.md` "Resources of a tag"):** the field has a tag struct of kind 2 at its place; the k-th such struct (in list order) is the file entry's resource k. Verified on every tag with resources (19,694 Forge, 20,780 Xbox), with every field always pointing to one kind named like the field. Former guess: the `index` in the resource's file name (`[index:kind]`) is the field's order among the tag's resource fields or tag struct entries of type 2. Known since M2 (`docs/format-notes.md` "Tags"): the resource at position k of the file entry's resource list is always named `[k:kind]`, and zonesets list resources by that index. So "resource k" means the same thing in names, the resource list and zonesets. | done      |
| Q10 | Does an equal `assetChecksum` mean identical stored data? AusarDocs: it hashes "the original file this was built from", "not always the same thing as the file stored in the module". Test on tags with copies in the Forge `globals` layers: compare `assetChecksum` equality with real data equality. Until answered, copy identity uses the data (§6.4.6). **Partly answered (M2, `docs/format-notes.md` "Copies"):** on all 214 Forge copies, `assetChecksum` equality and block checksum list equality both matched data equality, but only 1 copy was identical, so trusting `assetChecksum` is not proven. The data rule stays. Re-test on a full install, which has more copies. | after M2 (full install) |
| Q11 | What is the trailing data after the zoneset info in about 12% of tag headers (52-byte min/max records in materials and particles; other contents in `paas` and `sbsp`)? Until answered, it is kept as raw bytes and shown with its size. | M3, if field reading needs it |
| Q12 | ~~How should the header scan find tag references without a dependency entry?~~ **Answered (M5a survey, `docs/format-notes.md` "What references without a dependency point to"):** read the reference fields in the tag data: they are exact (class always matches the target), while listed names are ambiguous for most indexed targets (one path, several classes). Most such references (Forge 97.6%, Xbox 93%) point to tags outside the indexed modules. Still open: why the game leaves the dependency entry out. | done (why: open) |
| Q13 | What are the remaining unknown fields of the tag header? Header: `unknown8` (`uint64`; one value is shared by thousands of tags, so probably per class or layout), `unknown18` (`uint32`), `unknown4b` (`int8`), `unknown4c` (`int32`; 0 to 0x41, not a marker for trailing data). Dependency `unknown14` (AusarDocs: always -1 so far). Data block `unknown4` (values 0, 1, 2, 3, 8). Tag struct `unknown12`. Data reference `unknown4`. string_id `unknown0` (varies: 1, 64, 129 in one tag). No milestone needs them: fields are read through the definitions (§5.3). `header-dump` shows them under their offset names. Investigate with a survey test (method above) when one is needed. (Zoneset `unknown4` is padding, solved in M2.) | when needed |

## 13. Technical design

**Workspace**

```
h5tags/
  crates/
    h5-module/     # modules, compression blocks, tag header, index, header scan (no XML, no MCP)
    h5-defs/       # definition loading, name parsing, field type table, custom-label rule
    h5-meta/       # tag reading: node tree, field paths, sections, checks, states, text/JSON renderers
    h5-bitmap/     # bitmap resource reading, decoding, preview sheet, export
    h5tags-core/   # config, builds, tools shared by CLI and MCP; layers: input (tools/args), domain (catalog, filters, query), presentation (tools/present, output) (decisions D21)
    h5tags-cli/    # clap CLI
    h5tags-mcp/    # rmcp stdio server: thin wrappers over the library crates
  docs/
    AGENT_GUIDE.md   # normative (§9a)
    format-notes.md  # source of every format fact (§3)
    decisions.md     # choices made where this PRD is silent (§0)
  README.md          # user setup (§14)
  tests/fixtures/    # synthetic/tiny samples only
```

**Crates** (check each is maintained before adding): `rmcp` (official Rust MCP SDK: server, stdio transport, image content), `tokio`, `serde`, `serde_json`, `schemars`, `toml`, `clap`, `binrw` or `byteorder`, `flate2`, `quick-xml`, `memmap2`, `regex`, `strsim` (did-you-mean), `thiserror`, `anyhow`, `tracing` + `tracing-subscriber`; MurmurHash3 x86 32 and x64 128 (crate or own implementation, tested against known values); bitmaps: a BC1–BC7 decoder crate, `image` (PNG/TGA/TIFF), a DDS crate.

**Rules**

- Logs go to **stderr only**; stdout carries the MCP protocol.
- Memory-map module files. Decompress only what a call needs. LRU cache of decompressed tags (size: `tag_cache_mb`, §6.1). A long-running process (the MCP server) keeps between calls the index, each module's parsed lists (module files are mapped again for each call, so none stays open and locked between calls), parsed definitions (parsed again when the file's size or modification time changes) and the tag cache (decisions D39).
- The library crates MUST NOT depend on MCP; everything is usable from the CLI.
- **OS-agnostic:** every crate MUST build and pass its tests on Windows, Linux and macOS. No OS-specific code outside an opt-in cargo feature (the only planned one: `xg.dll` de-tiling, backlog 8). File system paths use `std::path`; tag paths (`\`-separated game data) are never treated as file system paths. Prefer pure-Rust dependencies (no C toolchain needed). A CI workflow (GitHub Actions) builds and runs the unit tests on all three OSes from M1 on.

**Code conventions** (MUST; background in `docs/decisions.md` D7, D9, D21)
- **Domain apart from input and output.** Three layers:
  - *Input*: tool parameters and the parsing of raw arguments into domain values (`h5tags-core/src/tools/args.rs`). Bad input is rejected here, and only here, as a tool error with a hint.
  - *Domain*: `h5-module`, `h5-defs`, and `catalog` / `filters` / `query` in `h5tags-core`. They take typed values and return typed results or typed errors. They never format text, write hints, read CLI/MCP arguments or print.
  - *Presentation*: `tools/present.rs` and `output.rs` turn results into text and JSON.
- **One definition per rule (DRY).** A format fact, naming convention, filter, check name, marker or column name is written in exactly one place and used everywhere else. Before writing a rule, search for an existing one. Markers and column names live in `output::marker` / `output::col`, and a test checks each is explained in `AGENT_GUIDE.md`.
- **One definition per tool** (D7): a tool's parameter struct is both its CLI command and its MCP arguments. Parameters shared by several tools are shared structs (`BuildArg`, `PageArgs`).
- **Text and JSON from the same data:** every result is built once and rendered twice. Never assemble the two separately.
- **Newtypes where values can be mixed up** (`GlobalId`, `FourCc`, `Size`), not for every number. Sentinel values (`-1`) are turned into `Option` or a named constant at the parsing boundary.
- **Names from the glossary (§4):** module, file entry, tag, resource, class, copy, compression block, data block, etc. The same name in code, output and docs.
- **Idiomatic Rust over checklists:** `match` on enums rather than trait hierarchies; no method-length or field-count quotas; no abstraction until a second use exists.
- **No wasted work in loops over tags or entries** (tens of thousands per call): no allocation per item (compare borrowed text, use the catalog's precomputed lowercase paths, compile matchers once per call), filter while iterating instead of collecting first, and compute display text only for the rows shown (D26).
- **Tests:** unit tests run on synthetic data built by the tests (never game files). Real-file tests are `#[ignore]`d and read `H5_DEPLOY`. New format parsing (M2 on) is written test-first: the expected facts become the tests, then the code. Every PRD §9 check is an automated test.
- **Errors:** library code doesn't `unwrap` or panic on file contents; malformed data becomes a typed error with the location (module, entry, block, field path). `unwrap` is for invariants the code itself guarantees.
- **Format facts:** every fact the code relies on has an entry in `docs/format-notes.md` stating how it was verified. Docs, comments and commit messages describe the files' format, never where game files were obtained.

## 14. User documentation (README)

`README.md` is a deliverable of M4 (MCP setup) and is updated at each later milestone. It MUST contain, in this order:

1. **What it is**, in two or three sentences, and the supported builds.
2. **Requirements:** Windows, Linux or macOS (x64 or ARM64; CLI and MCP server on all three; `xg.dll` de-tiling, backlog 8, is Windows x64 only), the game files for each build, and a copy of Lord Zedd's H5Tags (link; the `tags/` and `tag_resources/` folders).
3. **Install:** download a release binary, or build with `cargo build --release`.
4. **Configure:** a complete `h5tags.toml` example (§6.1) with every key explained, plus the `--deploy/--defs` single-build shortcut.
5. **Check it works** without any AI client: `h5tags list-modules` and `h5tags search-tags assault_rifle`, with what correct output looks like.
6. **Connect it to an AI client**, one subsection each:
   - Claude Code: `claude mcp add h5tags -- <path to h5tags-mcp.exe> --config <path to h5tags.toml>`, plus how to choose the scope (`--scope user` for all projects, default local scope for the current project only) and how to check it with `claude mcp list`.
   - Claude Desktop: the `mcpServers` entry for `claude_desktop_config.json`.
   - Other MCP clients: it's a stdio server; command and arguments as above.
7. **First steps:** three example questions to ask the agent, and a pointer to `docs/AGENT_GUIDE.md`.
8. **Troubleshooting:** server not listed (path, restart the client), "no definition" errors (wrong `defs` path), slow first `find_references(in)` (header scan, §6.2), where logs go (stderr; how each client shows them).
9. **Legal:** what is not bundled and why (§3); credits to AusarDocs, Lord Zedd and Reclaimer as references.

The repository MAY also contain a `.mcp.json` at the root that registers a local debug build for development (project scope), so the server is available to Claude Code while working on the code itself.
