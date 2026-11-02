# h5tags — guide for AI agents

How to use the `h5tags` MCP tools to answer questions about Halo 5 tags, and how to read what they return. This guide is **normative**: the tool's text output MUST match it (PRD §9a). If they disagree, the PRD wins and this guide is fixed.

## 1. Mental model

- **Build**: one version of the game with its own files, e.g. `forge` (Halo 5: Forge, PC) or `xbox` (Halo 5: Guardians, Xbox One, which also has campaign content). Results say which build they come from when more than one is configured. Never mix values from different builds without saying so.
- **Module** (`.module`): an archive in a build's deploy folder. Modules layer on top of each other, so the same tag can have **copies** in several modules.
- **Tag**: one piece of game data (a weapon, a projectile, an effect...). Identified by a **global ID** (`0x00A1B2C3`, the same in every module) and a **tag path** (`objects\weapons\rifle\assault_rifle\assault_rifle`).
- **Class**: the tag's type, a 4-character code + a name: `weap` = `weapon`, `proj` = `projectile`. **Display name** = tag path + `.` + class name.
- **Definition**: the XML (Lord Zedd's H5Tags) that names and types a class's fields. Field names are the original developer names.
- Fields are grouped by **sections** (headings), **structs** (fixed groups), **arrays** (fixed repeated groups) and **tag blocks** (lists of entries: `barrels[0]`, `barrels[1]`).
- **References**: tags point to other tags. Behaviour is usually spread along a chain: weapon → projectile → damage effect.
- **Class sections**: many classes include their parent classes' fields first. A `dspn` (dispenser) is `OBJECT` fields, then `DEVICE`, then `DISPENSER`. **A class's own fields are usually in its last class section.**

## 2. Workflow

1. **Find** the tag: `search_tags` if you know part of a name; `browse` to explore a folder; `search_definitions` if you know what you want but not which class controls it.
2. **Orient**: `outline(tag)` before reading anything large. It's cheap and shows sections, field paths and entry counts.
3. **Read** only what you need: `read_tag(tag, section=...)` or `read_tag(tag, path=...)`.
4. **Follow references** when the answer isn't in this tag: `find_references(tag)`, optionally `recursive=true`.
5. **Compare** with `query_field` (one field across a class) or `diff_tags` (two tags of the same class), at `detail=brief`.
6. **Look up meaning** with `get_definition(class, path=..., detail=full)` when a field's name isn't self-explanatory.

### Which tool
| Question | Tool |
|---|---|
| "Find the assault rifle" | `search_tags("assault_rifle")` |
| "What's in this folder?" | `browse(path)` |
| "Which class controls weapon heat?" | `search_definitions("heat")` |
| "What does this tag contain?" | `outline(tag)` |
| "Show the firing settings" | `read_tag(tag, section=...)` or `read_tag(tag, path=...)` |
| "What does field X mean / what values can it take?" | `get_definition(class, path=..., detail=full)` |
| "What does this weapon use?" | `find_references(tag, direction=out, recursive=true)` |
| "What uses this projectile?" | `find_references(tag, direction=in)` |
| "Magazine size of every weapon" | `query_field("weap", path, ...)` |
| "Assault rifle vs SMG" | `diff_tags(a, b)` |
| "Which maps load this?" | `which_maps(tag)` |
| "What assets does this tag have, and how big are they?" | `list_entries(tag=...)` |
| "Biggest meshes / all assets of a kind" | `list_entries(kind="mesh resource", sort=size)` |
| "Bitmaps with 'magnum' in the name" | `search_tags("magnum", class="bitm")` |
| "Every bitmap this weapon uses" | `find_references(tag, recursive=true, class="bitm")` |
| "What kinds of assets exist?" | `list_resource_kinds()` |
| "Is this tag the same in every module?" | `tag_copies(tag)` |
| "Which tags mention 'plasma'?" | `search_values("plasma")` |
| "What's in the Xbox build but not Forge?" | `compare_builds("xbox", "forge")` |
| "What does this texture look like?" | `view_bitmap(tag)`: read the text summary first; the sheet shows each channel separately |
| "What would it take to make X behave like Y, and what else would change?" | §2.1 below |

A tool named in this guide but missing from your tool list isn't available in this version of h5tags yet. Use the tools you have, and tell the user what the missing one would have answered; don't guess it.

### 2.1 What a change affects

When the user asks what it would take to change a tag, or what a change would also touch (the HUD, pickups, other modes…), work outward from the tag:

1. **Compare with a tag that already does what the user wants.** Read the same paths in both, at `detail=brief` (`diff_tags(a, b)` does it in one call). The fields that differ are the candidates. Read one struct or tag block at a time, not whole tags.
   - Example, from one session: to turn a magazine weapon into a battery weapon, the battle rifle was compared with the plasma pistol. The battery weapon's barrel points to no magazine (`magazine  -1 (none)`), uses `rounds per shot  0`, and adds `age generation.age generated per round` for each shot; the weapon sets `flags#3` `cannot fire at maximum age`. Read these yourself; don't reuse the example's values.
2. **Check the pattern on a third tag.** Two tags can differ in many ways that have nothing to do with the question. Look at another tag with the same behaviour (another battery weapon), or at the field across the whole class (`query_field`), before calling a field the cause.
3. **Follow what the tag points to.** The systems around a tag are its references: HUD screens (`player interface.hud screen reference`), projectiles, effects, sounds. Outline them, and read the parts that name the behaviour. Their string_ids often say what they switch on (`battery_remaining_expression`, `is_low_ammo_decision`); `search_values` finds such names across tags.
4. **Find what points to the tag** (`find_references(direction=in)`): game types, Forge palettes, scenarios and other tags that use it. A change reaches every one of them.
5. **Search the definitions for the behaviour's words** (`search_definitions("drop")`, `"pickup"`, `"ammo"`) to find fields elsewhere that take part, for example the settings a dropped or picked-up weapon keeps.
6. **Separate what the tags say from what the engine does.** Tags hold settings; the game's code decides what happens with them at runtime. Whether a battery level is kept when a weapon is dropped, how a value syncs online, or how the AI uses it may not be stored in any tag. Show the related fields and what other tags set, say clearly that the conclusion is inference, and suggest testing it in game.
7. **Answer with a change list:** for each tag, the field path, the current value → the proposed one, and why (which comparison shows it); then what else may be affected, and what stays unknown. h5tags only reads tags; say that the change itself is made with a tag editor.

## 3. Budget rules

- **Never read a whole large tag** (`scnr`, `effe`, `vehi`, `bipd`, `char`, `gint`, `unit`, `weap`). Call `outline` first, then read sections or paths.
- **The size guard:** a whole `read_tag` (or `get_definition`) bigger than the limit (about 3,000 tokens by default) returns the outline instead, with `hint: whole read ≈ N tokens (L lines); …`. Read a `path=` or `section=` from that outline. Use `confirm_large=true` only when the answer truly needs every field of the tag (for example, the user asks for all of it): the result is then preceded by `warning: large result, ≈ N tokens`. Tell the user when you do.
- `detail=normal` is the default. Use `brief` for anything covering many tags or many entries. Use `full` only for the few fields you're explaining or about to change.
- Use `[*]` paths (`barrels[*].rounds per second`) instead of reading each entry separately.
- Don't re-read what you already read in this conversation, unless something changed or you need a different detail level.
- Follow paging hints instead of raising `limit` a lot.

## 4. Reading the text output

### 4.1 Layout
```
# weap objects\...\assault_rifle.weapon  0x00A1B2C3  build=forge  copies: 3 (identical)   ← tag line
## WEAPON                                          ← class section (inherited parent classes come first)
bounding radius  real  0.1708791                   ← field of the main struct: no indent
### barrels  block[2]  {primary, secondary}  showing [0] primary   ← struct / array / tag block heading
  rounds per second  bounds  8.5..8.5              ← field: name, short type, value, units, extras
  trigger mode  enum16  2 charge  {normal|latch|charge|latch-autofire}
  #### Firing                                      ← explanation or group heading
  > Controls how each barrel fires. […]            ← first line of an explanation; […] = cut
  #### shots  block[3]  {first, second, third}  → path=barrels[0].shots
```
- `# ` = tag line (or definition line). `## ` = class section, nothing else. `### ` and deeper = explanations, group headings, structs, arrays and tag blocks; one more `#` per nesting level.
- **Indentation** = nesting: fields of the main struct have none; each struct, array or tag block entry adds 2 spaces. A line belongs to the nearest heading above it with less indentation.
- A section contains everything after its heading until the next heading at the same or a higher level.
- **Field line:** name, short type, value, then units/range and extras, separated by two spaces. Units come from the definition (`stops`, `seconds`); `[0.1-1]` is the definition's stated valid range. Numbers are exact (the shortest form that reads back to the stored value), never rounded.
- **Angles** (`real`, `bounds`, `euler2`, `euler3` fields of the definition's angle types) are stored in **radians**, though the definition labels them in degrees. The value is the exact stored number marked `rad`; the units then give it in the definition's own unit, converted to degrees and rounded to 3 decimals: `error angle  bounds  0.0026179939..0.04014257 rad  0.15..2.3 degrees (max_error_angle)`. Quote degrees to the user; JSON has the raw value plus `degrees`. `get_definition` shows the definition's unit only (degrees), since it has no stored value. Other fields with a degree unit (type `real`, no `rad`) store degrees as written; a few hold radians anyway, so if such a value looks like radians (small, and round once converted), say so instead of quoting it as degrees.
- **Group headings** (`#### Outlining`) are the editor's grouping of the fields below them; **explanations** are the definition's own notes. Neither holds data.
- `block[N] {labels}` = entry count and entry labels (max 8, then `+N more`). `showing [i]` = which entry is printed below. A label is the entry's labelling field (a name, or a referenced tag's file name); an entry without one is listed by its index (`{0, bullet_hit}`). When **no** entry has a label, the list is left out (`### magazines  block[1]  showing [0]`), and so is the label after a block index (`0 → magazines[0]`). JSON always has `labels`, with the index for an unlabelled entry.
- `→ path=...` = not expanded here; call again with that path.
- Repeated names among siblings get `#2`, `#3`: `flags#2` is the second field named `flags`.
- `resources: N (size)` on the tag line = the tag owns N resources (textures, meshes, sounds… stored outside the tag) totalling that size. Their contents are not loaded. `detail=full` lists each one (`resource [i]  <kind>  <size>`). Omitted when the tag has none.
- `copies: N (identical)` = all copies are the same. `copies: N (differ; reading <module>; use module= to choose)` = they differ; say which module's copy your numbers come from.

### 4.2 Brief format
`detail=brief` drops headings: each line is `<full field path>  <value>`, e.g. `barrels[0].rounds per second  8.5..8.5`. Two or more lines in a row from one tag block entry (or one entry of a `[*]` read) go under an `in <entry path>:` line, **indented**, with paths relative to it:
```
in barrels[0]:
  flags  0x00000380 {…}
  firing.rounds per second  29.999998..29.999998     ← = barrels[0].firing.rounds per second
```
An unindented line always has its full path. When a nested entry's group ends, the outer entry's next lines get their own `in` line again. Headings are dropped, but not what they say: a tag block gets a line of its own with its entry count (`automatic fire variations  block[8]  showing [0]`: only entry 0 follows; read `[*]` for all), a struct or block with a warning or error gets one too (`single wield penalties  struct  warning: definition GUID mismatch`), and a struct or block that wasn't expanded ends in `→ path=…`.

**Lists** (search results, `browse`, `list_*`, `query_field`, references) look like this:
```
build: forge                                      ← only when several builds are configured
total: 6380  total size: 1.3 GB                   ← all matches, not just this page; plus tool-specific totals
in objects\weapons\rifle\:                        ← prepend this to the name/path column of every row
type	name	kind	size	compressed	module        ← tab-separated header
resource	assault_rifle\assault_rifle.render_model[0:mesh resource!*]	mesh resource	6.1 MB	3.2 MB	any\levels\globals-rtx-1.module
… 6379 more (offset=1)                            ← next page
hint: …                                           ← only for unusual results; read it
```
Sizes use decimal units (`6.1 MB` = 6,100,000 bytes); JSON gives exact bytes.

### 4.3 Short type names
| Short type | Field types | Value shown as |
|---|---|---|
| `real` | real, real fraction, angle | `0.15` |
| `int8` `int16` `int32` `int64` | integers | `30` |
| `enum8` `enum16` `enum32` | one option | `2 charge` (number + label) |
| `flags8` `flags16` `flags32` | set of options | `0x00000005 {a, b}  (+N options)` |
| `bounds` | min/max pairs | `8.5..8.5` |
| `point2` `point3` `vec2` `vec3` `euler2` `euler3` `quat` `plane2` `plane3` `rect2` | multi-component | `(x, y, z)` etc. |
| `rgb` `argb` `hsv` `ahsv` | colours | `(r, g, b)` etc. |
| `string` | text | `"text"` |
| `stringid` | hashed name | `bullet` |
| `tagref` | reference to a tag | `<display name>  <global ID> [allowed classes]` |
| `blkidx` | index into a tag block | `1 → barrels[1] secondary` |
| `block[N]` | tag block with N entries | heading |
| `struct` | struct | heading |
| `array` | array | heading |
| `data` | data reference | `(N bytes)` |
| `function` | mapping-function curve | `(curve, N bytes)` (not decoded) |
| `resource` | pageable resource | `(mesh resource, 6.1 MB, not loaded)`: the resource's kind and size; `(…, not stored in this module)` if its data isn't in the module |
| `pad` | padding (hidden) | `(N bytes)` |
| `tag` | a class code | `weap` |

### 4.4 Options
- After an enum, `{a|b|c}` = all options; the chosen one is the label after the number.
- After flags, `{a, b}` = options that are **set**; `(+N options)` = how many others exist. Use `options=all`, or `get_definition`, for the full list.
- Options the definition marks as internal/unused are hidden unless `show_hidden=true`, but **the chosen enum value and every set bit are always shown**.
- For `[*]` reads, options appear once in a `legend:` block at the end.

### 4.5 Empty, missing and unreadable — read carefully
| Marker | Meaning | What you may say |
|---|---|---|
| `(empty)` | The tag has nothing there (0 entries, 0 bytes) | It's a fact about the tag |
| `(none)` | Null reference / no value | Fact |
| `(none set)` | Flags with no bits set | Fact |
| `(not in indexed modules)` | The referenced tag isn't in this build's modules | Say so; don't claim it doesn't exist in the game |
| `(unresolved)` | string_id couldn't be turned into text | Give the hash; don't guess the name |
| `(no definition for class …)` / `(no options defined)` / `(no option N; …)` / `(unnamed @0x..)` | The definition doesn't describe this | The value is real, its meaning is unknown |
| `(out of range; …)` | Block index points past the last entry | Fact about the stored value; its effect in-game is unknown |
| `(target block unknown)` | Block index whose tag block isn't in an enclosing struct (it may index into another tag) | The number is real; don't say what it points to |
| `(hidden)` / `(N hidden fields)` | Hidden by default (padding, internal fields) | Use `show_hidden=true` if relevant |
| `!! …` | **The tool failed to read this** | **Never** present those values, or their absence, as facts. Tell the user reading failed there and why |
| `(not read)` | Field after a read failure | Same as `!!` |
| `warning: definition GUID mismatch` | The tag stores a different struct type ID than the definition names here. The size fits and the checks passed; in every sample tag this was a labelling difference, not a misread. Shown by default (config `guid_check = warn`); with `guid_check = off` it isn't shown at all | Use the values, but mention the warning when precision matters |
| `!! definition GUID mismatch at <path> (definition <srcguid>, tag <guid>; guid_check = enforce)` | The same `definition GUID mismatch`, with the config set to `guid_check = enforce`: the struct or tag block is treated like a size mismatch, and its values are not shown | The tool refused to read them; say so, and don't state their values or their absence |
| `!! the definition doesn't match the tag header at N place(s) …` (below the tag line) | The definition places fields where the tag's own lists don't | Values may be misplaced: don't state them as facts |

### 4.6 Other results: what they mean
| Result / column | Meaning | What you may conclude |
|---|---|---|
| `hint: …` | A rare or partial result explaining itself, often with the next call to make | Follow it, or tell the user what it says |
| `by class: mat  100, wcfg 31, … (+N more)` (`search_tags`) | How the hits split by class, most first | Narrow with `class=` instead of paging through everything |
| `total: 0` + `hint: … similar names in tag paths: …` | Nothing matched; these real names are close to the query | Retry with one of them; don't conclude the tag doesn't exist |
| `total: 0` + `hint: no tag matches every filter; …` | The query matches tags, but `class=`/`module=` removed them | Loosen the filters |
| `… N more (offset=X)` | More rows exist; call again with that offset | Never say "that's all" before the last page |
| `in <prefix>:` | Rows below are shortened; prepend the prefix | — |
| `total` (lists) | How many rows match in all, not just this page | Use it for "how many" answers |
| `list_entries` entry type `tag` / `resource` / `file` | A tag; an asset owned by a tag (texture data, mesh, sound…); another file (e.g. `loadmanifest`) | Resources are part of their owner tag, not separate tags |
| `list_entries` sizes | Uncompressed and compressed (on-disk) size of the entry | Size only; contents are not loaded |
| `hint: the tag has N copies: <module> (this), <module>, … (+K more); …` (`list_entries` with `tag=`) | The same tag (same global ID) is stored in several modules, one copy each; modules layer on each other, so a later layer can hold an updated version. The rows come from the copy marked `(this)` | Say which module the rows come from; pass `module=` with one of the named modules to see another copy. Copies usually differ; don't call them duplicates. This is where the tag is stored, not which tags use it (that is `find_references`) |
| `(not stored in this module)` (`list_entries` compressed column) | The module describes this data and its size, but doesn't contain it. Where it is stored is unknown | Its size is real; don't say the data is missing from the game |
| `list_modules` status `ok` | Every structural check of the module passed | — |
| `list_modules` status `!! incomplete file: …` | The module file is shorter than its own file list says (e.g. a copy that didn't finish); entries past the end can't be read | Tell the user; a fresh copy of the file usually fixes it. Tags in it may fail to read |
| `list_modules` status `!! damaged: …` / `!! not indexed: …` | A structural check failed, or the file couldn't be read as a module at all | Same: the tool's problem with this file, not a fact about the game |
| Empty result with `hint: … The class exists (it has a definition) …` | No tags of that class are in the indexed modules | Don't say the game has none; its tags are in modules that aren't indexed |
| `browse` rows ending in `\` | Subfolders, with the number of tags under each (recursively) | — |
| `list_resource_kinds` | Each asset kind with its count and total size | — |
| `# <class> <name> (definition)` (`get_definition`, `outline(class=)`) | The class's schema from its definition alone: no tag data, no values | Field types, units, options and help apply to every tag of the class |
| `get_definition` `block  (max N)` / `tagref  [proj]` / `blkidx  → barrels` / `(N options)` | A tag block and its maximum entry count; the classes a reference allows; the block an index points into; an enum or flags field with more options than shown (`options=all` lists them) | — |
| `hint: whole read ≈ N tokens (L lines); read a path= or section=, or repeat with confirm_large=true` (`read_tag`, `get_definition`) | The whole read was over the size limit, so you got the depth-1 outline instead (no values). N and L are the size of the whole read (tokens ≈ characters ÷ 4) | Nothing about the tag's values; read the part you need. The outline's paths are real |
| `warning: large result, ≈ N tokens (L lines)` (second line) | You asked for a whole read above the limit with `confirm_large=true`; the result is complete | The values are as normal; mind your context budget |
| `search_definitions` row order | Hits where the query is a whole word (`age` in "age misfire start", "cannot fire at maximum age") come first; after them come hits that only contain it inside a word ("damage", "percentage", "ages"), each group in definition order | The first rows are usually the ones you want; the later ones rarely need paging through |
| `outline` `(N fields)  path=…` | How many fields the struct or tag block holds directly, and the path to read it with | Read only the part you need: `read_tag(path=...)` |
| `outline(tag=)` `block[N] {labels}` | The tag's entry count and labels; nested blocks are shown as entry 0 has them | Other entries can have different counts |
| `definition_status` `has definition` | Whether a definition XML exists for the class | No → that class's tags can't be read field by field, only listed and followed by references |
| `definition_status` mismatches + `checked N of M tags` | Size or GUID mismatches found so far between the definition and real tags of that class, and how many of its tags have been checked (kept across sessions) | Mismatches → values from that class may be wrong where flagged. "None found" only covers the N tags checked, not the whole class |
| `tag_copies` `identical` / `differ` | Whether every copy of the tag has the same stored data (checked on the data itself) | `differ` → values can depend on the module; say which copy you used |
| `compare_builds` `only in A` / `only in B` / `different` | By global ID: tags present in one build only, or in both with different stored data | `different` means the data differs, not necessarily a gameplay change; diff the tags to see what |
| `find_references` `(see above)` | That tag was already expanded earlier in the tree | Not a new reference |
| `find_references` with `class`: `depth`, `via` | How many references deep the tag was found, and which tag points to it | The chain is real; what it does in-game needs reading the tags |
| `query_field` / batch `skipped` | Tags that failed to read, with the reason | They are missing from the results; say so if it matters |
| `(header scan built: N tags, T s)` | This call first had to scan all tag headers; later calls are fast | — |
| `reindex` result | What was rebuilt or dropped, and how long it took | — |

### 4.7 List columns
| Column | Meaning |
|---|---|
| `global ID` | The tag's global ID (`0x00A1B2C3`), the same in every module |
| `class` | Class code (`weap`). Codes can contain symbols and a trailing space (`mat `, `snd!`, `hsc*`) |
| `class name` | Class name (`weapon`) |
| `display name` | Tag path + `.` + class name |
| `name` | Display name, file name or folder (depends on the tool) |
| `copies` | In how many modules the tag is stored |
| `tags` | Number of tags (of a class, a module, or under a folder) |
| `definition` | Whether a definition exists for the class (`yes`/`no`) |
| `module` | Module path, relative to the build's deploy folder |
| `version` | Module format version (27 = Forge PC, 23 = Xbox One) |
| `load manifest` | Whether the module lists the tags a map loads |
| `status` | `ok`, or `!! …` (§4.6) |
| `type` | `tag`, `resource` (an asset owned by a tag) or `file` (e.g. `loadmanifest`) |
| `kind` | Class code for tags, asset kind for resources (`mesh resource`), `file` otherwise |
| `size` | Uncompressed size |
| `compressed` | Size stored in the module, or `(not stored in this module)` |
| `count` | Number of entries |
| `total size` | Sum of the uncompressed sizes |
| `field path` | Where a field is in its class (`barrels[*].trigger mode`; `[*]` = in every entry of that tag block). Use it as `path=` |
| `field type` | The field's definition type (`_field_short_enum`); `explanation`/`heading` for a matched heading |
| `matched` | What matched the query: `name`, `old name '…'` (an earlier name of the field), `label '…'`, `option …`, `help`, `units`, `explanation '…'`, `heading '…'` |

Results may come from the tool's cache (kept across sessions). A cached result is always identical to a fresh one, so you never need to "refresh" it. Only `reindex` forces a rebuild, and the tool keeps itself up to date when game files change.

### 4.8 Tag internals you will see
Every tag stores, besides its field values, a few lists that say what its fields point to. `read_tag` uses them; this is what they mean when they show up:
- **Tag reference** (`tagref`): a field that names another tag by its global ID. The name comes from the index, or, for a tag that isn't indexed, from the tag's own list of names (then `(not in indexed modules)` follows). `(none)` = the field names no tag. `[proj]` = the classes the definition allows there. A tag can reference tags without listing them as dependencies; `read_tag` shows every reference field either way.
- **string_id** (`stringid`): a name stored as a hash. The text comes from the tag's own list of strings. `(unresolved)` = the hash isn't in that list: often an ID that is not a name at all (e.g. a sound event ID). Give the hash; don't guess the text.
- **Block index** (`blkidx`): a number that picks an entry of a tag block (`1 → barrels[1] secondary`). The block is the nearest enclosing one the definition names.
- **Data reference** (`data`): raw bytes stored with the tag (scripts, curves, tables). Only the size is shown.
- **Resource** (`resource`): an asset stored next to the tag, not inside it: a mesh, a texture, sound data. The kind and size come from the module's list of the tag's resources; the data itself is not loaded.
- **Size check:** before showing a struct, `read_tag` checks that the definition's size equals the size the tag stores. If not, it shows `!! definition size mismatch` instead of values: the definition doesn't fit this tag (usually a different game version).
- **Copies:** the same tag can be stored in several modules (update layers). `copies:` on the tag line says whether their data is identical and which copy you are reading.

## 5. Being accurate with the user

- **Values come from tools only.** Never fill in a value from memory or general Halo knowledge.
- **Field names and help text are hints, not proof.** When explaining what a field does, say whether that comes from the definition's help text, from the field name, or from your own inference.
- **Say which build and which copy** your numbers come from whenever more than one exists.
- Units and ranges come from the definition: usually right, but developer notes, not guarantees.
- **Not available in v1:** resource contents other than untiled bitmaps (no models, animations, sounds; you can see that they exist, their kind and size, but not what's inside), tiled (Xbox) textures, mapping-function curves, script contents, in-game display text, and any editing. Say so instead of working around it.

## 6. Arguments

- **Tag:** `[build:]<tag>`, where `<tag>` is a global ID (`0x00A1B2C3`), a tag path, or a display name. Examples: `0x00A1B2C3`, `xbox:objects\...\assault_rifle`, `...\assault_rifle.weapon`. If several tags match, the tool returns candidates: pick one, or ask the user.
- **Field path:** names joined by `.`, case-insensitive, spaces kept. `[i]` = tag block entry `i` (from 0); `[*]` = every entry; structs and arrays have no index. A tag block passed through **without** an index means entry 0 only (`barrels.firing` = `barrels[0].firing`): use `[*]` to get every entry. Names can contain dots and brackets (`peg threshold [0,1]`); copy them as `outline` shows them. Example: `barrels[0].firing noise`. On a typo, the error lists close names.
- **Section:** `section="EXPOSURE"` selects a section by heading. If the heading occurs more than once, you get the list of matches to choose from.
- **depth:** how many nested struct/array/tag block levels to expand (default 2; `outline` too). Raise it only for small sections.
- **confirm_large** (`read_tag`, `get_definition`): `true` = return a whole read even above the size limit (§3). Default `false`.
- **format** (every tool): `text` (default; what this guide describes) or `json` (the same facts as structured data).
- **Parameter names are checked:** a name the tool doesn't have is an error (`!! error: read_tag has no parameter 'paths'`) whose hint lists the real ones; nothing is ignored silently.
- **Filters** mean the same on every list tool that has them, and all given filters must match:
  - `query` + `mode` (`default` = path contains; `regex`, e.g. `magnum|pistol`; `exact`)
  - `class` (class code or name)
  - `kind` (class, resource kind like `mesh resource`, or `file`)
  - `module` (part of the module path)
  - `min_size` / `max_size` (`500KB`, `10MB`)
  - `offset` / `limit`

  Combine filters in one call instead of fetching a big list and filtering it yourself.

## 7. Example

> User: "How fast does the assault rifle fire, and what does it shoot?"

1. `search_tags("assault_rifle", class="weap")` → one result.
2. `outline(<global ID>)` → under `## WEAPON`: `barrels block[1]`.
3. `read_tag(<global ID>, path="barrels[*].rounds per second", detail=brief)` → `barrels[0].rounds per second  8.5..8.5`.
4. `find_references(<global ID>)` → a `proj` tag at `barrels[0].projectile`.
5. Answer: 8.5 rounds per second (minimum and maximum are equal, so the rate doesn't ramp up), firing `…bullet.projectile`. Offer to read the projectile's damage next.
