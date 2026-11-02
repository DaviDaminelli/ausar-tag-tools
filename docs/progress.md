# Progress and measurements

What each milestone achieved, checked against the PRD §9 "done when" criteria, with the measured numbers. Newest milestone last; numbers are kept as recorded (they are a baseline for later changes).

Measured on the developer's PC (Windows 11), with random sample modules, containing:

- **Forge sample:** 3 modules (v27), 87,221 file entries, 62,411 tags, 202 classes.
- **Xbox sample:** 6 modules (v23), 122 file entries, 73 tags.

Times are from an optimized build unless marked "debug".

## M1: module reader, index, search and lists (2026-10-31 / 11-01)

**Done-when checks**

| Check                                                                       | Result                                                                                                                                                                                   |
| --------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Every module indexes without errors                                         | Passed on every sample module (3 Forge, 6 Xbox). Full deploy folder: pending (D3)                                                                                                        |
| Structural cross-checks per module                                          | Passed on every module: data area ends at the end of the file, header fields agree, every list and data range inside the file, sections and block counts add up, no duplicate global IDs |
| Every compression block of every file entry decompresses to its stated size | Passed: all 87,221 Forge entries (2.1 GB decompressed) and 122 Xbox entries (602 MB). 2 Xbox entries are described but not stored in their module (D17)                                  |
| The index lists each tag's resources                                        | Passed                                                                                                                                                                                   |
| 5 sample searches return what the user expects                              | Passed (user, 2026-11-01)                                                                                                                                                                |
| CI on Windows, Linux and macOS                                              | Pending: the first run happens when the repo is on GitHub                                                                                                                                |

**Measurements**

| What                                                     | Number                                         |
| -------------------------------------------------------- | ---------------------------------------------- |
| Index, cold build (Forge, 87,221 entries)                | 0.18–0.25 s (target: < 10 s on a full install) |
| Index, warm start (load cache + check stamps)            | 0.06–0.13 s (target: < 1 s)                    |
| Index cache size (Forge)                                 | 11 MB                                          |
| Index, cold build (Xbox, 122 entries)                    | 2.5 ms                                         |
| Decompress every entry (Forge, 2.1 GB, parallel)         | about 1.0 s                                    |
| Session open (load index, build catalog)                 | 0.10 s                                         |
| `search_tags` over 62,411 tags                           | 5.0 ms                                         |
| `browse` (top level)                                     | 2.7 ms                                         |
| `list_resource_kinds`                                    | 1.2 ms                                         |
| `list_entries` (size-sorted, 10 rows)                    | 0.25 ms                                        |
| Real-file tool test: `list_entries(tag)` on sampled tags | 644 tags, all resolved                         |

**Loop optimization (D26)**, same machine, same data:

| Call                  | Before  | After   | Speed-up |
| --------------------- | ------- | ------- | -------- |
| `search_tags`         | 14.7 ms | 5.0 ms  | 2.9×     |
| `search_tags` (regex) | 15.0 ms | 5.5 ms  | 2.7×     |
| `browse`              | 7.4 ms  | 2.7 ms  | 2.7×     |
| `list_resource_kinds` | 3.2 ms  | 1.2 ms  | 2.7×     |
| `list_entries`        | 1.3 ms  | 0.25 ms | 5.2×     |

**Format findings** (details in `format-notes.md`): v23 = Xbox module layout; module metadata is uncompressed (indexing needs no decompression); file names carry the class name; resources are named `[index:kind]`; definition file names trim trailing spaces (`mat `) and replace characters not allowed in file names (`hsc*` → `hsc_`); entries can be described but not stored in their module; an incomplete module copy is detected exactly (missing bytes and entries).

## M2: tag decompression and tag headers (2026-11-01, passed: accepted by the user)

**Research before the plan**, on every sample tag (62,620 Forge, 73 Xbox):

- Tag header version 27 in both builds (Q1 answered).
- 1,558,134 Forge and 77,236 Xbox data blocks, all in the tag data section.
- Zoneset info matches its documented layout in every tag.
- 88.4% of Forge tags: documented header sections add up exactly; 11.6%: undocumented trailing data (Q11).

**Step 1: MurmurHash3**

- x86 32-bit: all published reference values, and `"__default__"` → `0x9B555AD2`.
- x64 128-bit: **179,161 of 179,161** Forge compression block checksums match (the first 64-bit half), proving both the hash and every block's decompression byte for byte.

**Step 2: reading one section of an entry**

- **87,221 Forge and 114 Xbox entries:** the three sections read separately equal the whole entry.
- Reading what a tag read needs (header + tag data) instead of whole entries: **847 MB instead of 2,138 MB, 2.6 s instead of 5.8 s** for every Forge entry (2.5× less data, 2.2× faster).

**Step 3: tag header parser and cross-checks**

Every tag of both samples, through `TagHeader::parse`, `check` (against the file entry) and `check_tag_data` (fields in the tag data section):

| What                                   | Forge              | Xbox         |
| -------------------------------------- | ------------------ | ------------ |
| Tag headers parsed                     | 62,620 (172.3 MB)  | 73 (13.4 MB) |
| Read + parse + check, every tag        | 1.42–1.51 s        | 44–45 ms     |
| Parse errors                           | 0                  | 0            |
| Check failures                         | 0                  | 0            |
| Tags with trailing data (Q11)          | 7,237              | 31           |
| Tag references checked                 | 888,569            | 2,883        |
| … without a dependency                 | 502,754            | 1,329        |
| … of those still naming a tag (Q12)    | 42,473             | 575          |
| Dependencies                           | 193,052            | 1,316        |
| … pointing at indexed tags, same name  | 154,227 of 154,237 | 32 of 32     |
| Distinct global IDs not in the samples | 12,652             | 571          |
| Tags with non-ASCII strings            | 1 (Windows-1252)   | 0            |

Two PRD facts corrected along the way (format-notes "Tags"): dependency -1 doesn't mean a null reference, and string tables are Windows-1252, not UTF-8. New check: a tag reference field's +0x08 is its listed name's length (true for every reference).

**Step 5: copy identity (Q10)**

Every entry with copies in several modules (same kind and name), each copy compared with the first one by its decompressed bytes. Forge sample; the Xbox sample has no copies.

| What                                          | Number                                  |
| --------------------------------------------- | --------------------------------------- |
| Entries with copies                           | 195 tags (181 × 2, 14 × 3), 5 resources |
| Copies compared                               | 214, in 0.26 s                          |
| Identical copies                              | 1                                       |
| `assetChecksum` agrees with data equality     | 214 of 214                              |
| Block checksum list agrees with data equality | 214 of 214                              |
| Copies with another global ID                 | 0                                       |

Q10 is partly answered: no counterexample, but a single identical copy can't prove that an equal `assetChecksum` means identical data. Copy identity keeps comparing block checksums (PRD §6.4.6). Re-test on a full install.

**Step 6: `header-dump`**

| What                                                          | Number                           |
| ------------------------------------------------------------- | -------------------------------- |
| Small tag (carbine sight: 3 dependencies, 18 tag structs)     | 128–133 ms                       |
| Largest Xbox header (5.2 MB `sbsp`, 34,000 rows, 6.6 MB JSON) | 195 ms JSON, 173 ms text         |
| A call that reads no tag (`list_classes`), for comparison     | 119–130 ms                       |
| Dependencies without a listed name (both samples)             | 0                                |
| Dependencies listed with the class suffix                     | 0 of 154,227 Forge, 0 of 32 Xbox |

Times are a release build, output to `/dev/null`, both builds configured.

**M2 done-when checks**

| Check                                                  | Result                                                                                                                                                                                     |
| ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Every tag of every sample module: section sizes add up | Passed: 62,620 Forge and 73 Xbox tags, 0 parse errors, 0 check failures (header sizes equal the file entry's; lists, string table, zoneset info and trailing data fill the header section) |
| Dependencies print as readable display names           | Passed: every dependency of both samples has a listed name, so `header-dump` names each one (from the index when indexed, else listed name + class). Seen on real tags of both builds      |

**Zoneset fields decoded** (after the first `header-dump` review; every tag of both samples):

| What                                                                  | Forge              | Xbox           |
| --------------------------------------------------------------------- | ------------------ | -------------- |
| Zonesets                                                              | 77,153             | 85             |
| Zoneset names resolved from the tag's own string_ids                  | 77,153             | 85             |
| Zoneset tags whose zoneset name resolves                              | 327,459 of 327,459 | 1,318 of 1,318 |
| `unknown4` not 0                                                      | 0                  | 0              |
| Resource indices listed (+0x08 count, +0x10 offset)                   | 24,601             | 43             |
| Tags with resources whose zonesets list each resource, and only those | 19,495 of 19,495   | 32 of 32       |
| Zoneset sections whose size is exactly their contents                 | 62,620 of 62,620   | 73 of 73       |
| Resources named `[k:kind]` at position k of the resource list (Q9)    | 24,601 of 24,601   | 43 of 43       |

New check `zoneset` (in `TagHeader::check`): 0 failures on both samples.

## Before M3: larger samples (2026-11-01)

The samples grew before M3's research step. **Forge:** 10 modules, adding two maps (each with its two update layers, two of them empty) and a second update-layer copy of the globals module. **Xbox:** 9 modules, adding a shaders module, an older globals update module, and the base globals module (1.1 GB, verified against the archive's CRC-32 before use). Every M1/M2 real-file check was run again on them:

| Check                                    | Forge                                                                   | Xbox                               |
| ---------------------------------------- | ----------------------------------------------------------------------- | ---------------------------------- |
| Modules: structural checks               | 10 of 10 pass (incl. two empty layers)                                  | 9 of 9 pass                        |
| Entries decompressed                     | 88,307 (2.8 GB) in about 2 s, 0 errors                                  | 85,366 (2.8 GB) in 1.5 s, 0 errors |
| Stored block checksums matching (h1)     | 182,109 of 182,109 (+93 entries without a block list, which store none) | (v23: none stored)                 |
| Sections read separately = whole entries | all entries                                                             | 85,357 of 85,357                   |
| Tag headers parsed, check failures       | 62,976, 0 parse errors, 0 failures                                      | 57,537, 0 parse errors, 0 failures |
| Resources named `[k:kind]` at position k | 25,332 of 25,332                                                        | 27,822 of 27,822                   |
| Zoneset names resolved                   | 77,531 of 77,531                                                        | 67,236 of 67,236                   |

- **One fact corrected:** "the zonesets list every resource" fails for 31 bitmaps in one Forge update module, so the `zoneset` check no longer requires it (format-notes "Tags").
- **One module moved:** a v23 module (the Xbox layout) was in the Forge folder by mistake; it is now in the Xbox folder. The module version in the header tells the two builds apart.

## M3: `read_tag`, definitions, outline (2026-11-01, passed: accepted by the user)

**Step 1a: definition parsing and the field type table**

| What                                   | Number                                 |
| -------------------------------------- | -------------------------------------- |
| Definition files parsed                | 307 of 307 (289 classes, 18 resources) |
| Fields                                 | 39,735, parsed in 56 ms                |
| Field types used / in the table        | 61 / 61 (none unknown)                 |
| Definitions whose size can be computed | 307 of 307                             |

**Step 0: the survey** (decisions D31; one run per sample, about 1.5 s each)

| What                                                | Forge                   | Xbox                    |
| --------------------------------------------------- | ----------------------- | ----------------------- |
| Tags / classes with a definition                    | 62,976 / 210            | 57,537 / 214            |
| Classes without a definition                        | 0                       | 0                       |
| Main struct size = definition size                  | 62,976 of 62,976        | 57,518 of 57,537        |
| Tag blocks: size = entry count × entry size         | 790,750 of 790,750      | 975,247 of 975,249      |
| GUID matches in the Windows byte order / as written | 1,880,332 / 0           | 2,408,432 / 0           |
| Custom struct GUIDs matching the definition         | 5,353 of 5,353          | 5,525 of 5,525          |
| Tag block GUIDs matching the definition             | 1,874,979 of 1,878,819  | 2,402,907 of 2,405,390  |
| Main struct GUIDs per class                         | exactly 1 (210 classes) | exactly 1 (214 classes) |

Answers: the field type table is right (every Forge main struct and checked block fits); arrays are single-instance (Q7); `srcguid` is a Windows GUID (Q3). The 19 Xbox misfits are 5 UI/globals classes whose Xbox layout differs from the definitions (for M6). The GUID check stays `warn` (D32).

**Step 1b: names and definition rules** (all 289 class and 18 resource definitions)

| What                                                     | Number              |
| -------------------------------------------------------- | ------------------- |
| Field and option names parsed, display names clean       | 68,781 of 68,781    |
| Help / units / ranges                                    | 9,002 / 3,140 / 329 |
| Old names `{…}` (names with two)                         | 2,759 (551)         |
| Labels `X&Y`                                             | 326                 |
| Hidden (`!`) / labels entries (`^`)                      | 4,431 / 1,134       |
| Customs: class section / group heading / widget          | 93 / 533 / 352      |
| Definitions starting with vtable space + internal struct | 289 of 289          |
| Mapping-function structs                                 | 529                 |
| Fields renamed `#2`, `#3`… to be unique among siblings   | 1,774               |

**Step 2a: the tag reader** (`h5-meta`; every tag of both samples read with its definition)

| What                                               | Forge                    | Xbox                             |
| -------------------------------------------------- | ------------------------ | -------------------------------- |
| Tags read                                          | 62,976                   | 57,537                           |
| With no problem and no divergence (checks 1 and 4) | **62,976 (100%)**        | 57,518 (19: the 5 known classes) |
| Tag blocks / structs and arrays read               | 1,878,819 / 2,924,539    | 2,405,230 / 3,298,562            |
| Header ↔ definition divergences (check 1)          | 0                        | 0                                |
| GUID mismatches, values kept (`warn`)              | 34,121 at 64 fields      | 46,993 at 60 fields              |
| Time, every tag (release, incl. decompression)     | 18.2 s (0.29 ms per tag) | 22.8 s                           |

Speed-up in the same step (D26): the first version took 69.4 s on Forge. Path steps are now computed once per definition (`Field::step`), nodes borrow their names, and field paths are formatted only for problems and divergences: 69.4 → 18.2 s (3.8×).

**Step 2b: values** (same samples; checks 5 and 6, and Q9)

| What                                                 | Forge                             | Xbox                              |
| ---------------------------------------------------- | --------------------------------- | --------------------------------- |
| string_ids: resolved / zero / unresolved (fields)    | 1,196,020 / 584,019 / 31,611 (23) | 1,708,400 / 796,978 / 43,250 (15) |
| Enum values naming no option (fields)                | 77,564 of 3,643,636 (57)          | 76,503 of 3,709,446 (56)          |
| Block indices: none / no target found / out of range | 965,929 / 27,469 / 238,118        | 1,689,991 / 54,449 / 494,017      |
| Tag blocks over their `max`                          | 0                                 | 0                                 |
| Reals not finite (fields)                            | 110 of 12,758,290 (4)             | 111 of 13,891,904 (20)            |
| Tags whose resource structs = resources (Q9)         | 19,694 of 19,694                  | 20,780 of 20,780                  |
| Resource fields pointing to more than one kind       | 0 of 18 places                    | 0 of 12 places                    |
| Time, every tag                                      | 17.1 s                            | 19.4 s                            |

Q9 answered: the k-th resource struct of a tag is its resource [k].

**Step 3: field paths and sections** (all 289 class definitions)

| What                                             | Number           |
| ------------------------------------------------ | ---------------- |
| Field paths that parse back to their field       | 36,450 of 36,450 |
| Path steps containing `.` / `[…]`, handled       | 125 / 157        |
| Explanation headings found as sections           | 1,555 of 1,555   |
| Time, every path and heading of every definition | 0.17 s           |

**Step 4a: `read_tag`** (text and JSON from one view; AGENT_GUIDE §4)

| What                                                             | Forge                           | Xbox                            |
| ---------------------------------------------------------------- | ------------------------------- | ------------------------------- |
| Every tag rendered (text + JSON), failures                       | 62,976, 0                       | 57,537, 0                       |
| Time, every tag (modules opened once)                            | 21.7 s                          | 34.1 s                          |
| Text lines / text size / JSON size                               | 3,141,820 / 153.4 MB / 525.9 MB | 2,426,448 / 121.8 MB / 428.1 MB |
| Average / largest tag at depth 2 (lines)                         | 49 / 994 (Mantis bipeds)        | 42 / 994                        |
| `read_tag` on the assault rifle, one CLI call (session included) | 726 lines, 219–227 ms           |                                 |

The output matches the sample agreed with the user (D36).

**Step 4b: `get_definition`, `outline`, `search_definitions`**

| What                                                                   | Number                           |
| ---------------------------------------------------------------------- | -------------------------------- |
| Definitions rendered fully expanded (full detail, all options, hidden) | 289 of 289, 39,226 lines, 0.24 s |
| Their outlines (depth 3)                                               | 5,651 lines                      |
| `search_definitions` over all 289 definitions, one CLI call            | 267–275 ms                       |
| `outline` of the assault rifle / `read_tag` of it                      | 186 lines, 213 ms / 726 lines    |

**Step 5: `header-dump` field paths** (D29): with a definition, each tag reference shows its field path. On the assault rifle, all 197 references have one (e.g. `attachments[1].type`).

**M3 done-when checks** (PRD §9; Forge sample, 2026-11-01)

Checks 1–7 on every tag of the five classes (test `h5-meta` `m3_checks_on_the_five_classes`, 10.5 s):

| Class  | Tags  | 1 header ↔ definition | 2 references | 4 sizes | 5 string_ids hash back | 3 GUID mismatches (explained) | 6 implausible values (explained)        |
| ------ | ----- | --------------------- | ------------ | ------- | ---------------------- | ----------------------------- | --------------------------------------- |
| `weap` | 137   | 0                     | 0            | 0       | 6,453 of 6,453         | 292 at 2 fields               | 0                                       |
| `proj` | 194   | 0                     | 0            | 0       | 4,547 of 4,547         | 0                             | 0                                       |
| `bipd` | 21    | 0                     | 0            | 0       | 2,904 of 2,904         | 0                             | 0                                       |
| `vehi` | 63    | 0                     | 0            | 0       | 8,007 of 8,007         | 126 at 2 fields               | 0                                       |
| `effe` | 4,748 | 0                     | 0            | 0       | 61,335 of 61,335       | 22,587 at 1 field             | 76,058 enum values, 219 indices, 79 NaN |

Every GUID mismatch and implausible value sits at a field recorded in format-notes ("GUIDs", "Values") with its cause: zero unexplained failures. Check 7 (whole-build sweep): every tag of every class reads and renders (steps 2a and 4a).

Check 9, the user's values from playing (test `h5tags-core` `real_spot_values`, read with `read_tag` as the AI reads them):

| The user said                   | Field                                                                                                | Tag value                             |
| ------------------------------- | ---------------------------------------------------------------------------------------------------- | ------------------------------------- |
| Battle rifle: 36 in a magazine  | `magazines[0].rounds loaded maximum`                                                                 | 36                                    |
| Battle rifle: 3-round burst     | `barrels[0].firing.shots per fire`                                                                   | 3..3                                  |
| Assault rifle: 36 in a magazine | `magazines[0].rounds loaded maximum`                                                                 | 36                                    |
| Covenant carbine: 20 rounds     | `magazines[0].rounds loaded maximum`                                                                 | 20                                    |
| Railgun: one shot at a time     | `magazines[0].rounds loaded maximum`                                                                 | 1                                     |
| At most 2 grenades of each type | grenade list `grenades[*].maximum count (…).Base Value`, all 3 types, campaign/firefight/multiplayer | 2 (bounds 0..6, combined by addition) |

5 of 5 match. The burst is `shots per fire`, not `rounds per burst` (0 on every weapon); the grenade bounds and addition combiner fit the user's note that an armor modification can raise the count.

### Simulated AI session: what it would take to make the battle rifle battery-powered (2026-11-01, Forge `any` globals)

CLI calls in text format, as the AI would make them; tokens ≈ characters / 4.

| Step                                                          | Calls | ≈ Tokens |
| ------------------------------------------------------------- | ----- | -------- |
| Find the two weapons (`search_tags`; "battle rifle" misses `br.weapon`) | 7 | 860 |
| Find battery fields (`search_definitions`; `age` matched 107 rows, inside "damage", "percentage") | 3 | 1,305 |
| `outline` depth 1 of both weapons                             | 2     | 2,072    |
| Sections heat, miscellaneous, more miscellaneous of both      | 6     | 1,182    |
| `barrels[0]` depth 1 of both                                  | 2     | 3,076    |
| Single fields: flags, age penalties, of both                  | 14    | 755      |
| `player interface` brief of both                              | 2     | 737      |
| HUD screens: outlines, property bindings                      | 5     | 2,055    |
| **Total**                                                     | 41    | 12,041   |

For comparison: the whole battle rifle read is 35,052 characters (≈ 8,763 tokens), and the whole weapon pair ≈ 17,500 tokens without the HUD. The HUD's battery logic was found only in the string_id names of `header-dump` (32,137 / 19,361 / 19,314 characters for three HUD screens, ≈ 17,700 tokens if read whole); the session grepped them instead, which an AI with the tools alone cannot do.

## M4: MCP server (2026-11-01, passed: accepted by the user)

### Step 1: what repeated calls cost (before any in-memory cache), Forge sample, release build

One session making the same calls 3 times (test `h5tags-core` `real_repeat_calls`, `repeated_calls`):

| Call                                               | First   | Repeated (mean of 2) |
| -------------------------------------------------- | ------- | -------------------- |
| `read-tag 0x00008593` (battle rifle, whole)        | 22.0 ms | 20.3 ms              |
| `read-tag 0x00008593 --path barrels[0] --depth 1`  | 19.7 ms | 19.5 ms              |
| `read-tag 0x000041B7 --path barrels[0] --depth 1`  | 11.3 ms | 10.3 ms              |
| `outline --tag 0x00008593 --depth 1`               | 20.4 ms | 19.5 ms              |
| `get-definition weap --path barrels`               | 1.7 ms  | 1.7 ms               |
| `search-definitions energy --class weap`           | 1.8 ms  | 1.8 ms               |
| `search-definitions rounds` (every class)          | 86.4 ms | 79.6 ms              |

Where the time of one read goes (a one-off breakdown test, removed once the caches made it measure cached reads):

| Tag (tag data size)            | Copy identity | Read + decompress | Parse definition | `read_tag` walk |
| ------------------------------ | ------------- | ----------------- | ---------------- | --------------- |
| battle rifle (11 KB, 3 copies) | 8.9 ms        | 8.6 ms            | 2.0 ms           | 1.7 ms          |
| largest `mode` (4,135 KB)      | (1 copy)      | 16.2 ms           | 0.9 ms           | 130.1 ms        |
| `unic` (2,749 KB, 3 copies)    | 9.2 ms        | 20.3 ms           | 0.3 ms           | 14.2 ms         |
| `jmad` (1,019 KB)              | (1 copy)      | 12.1 ms           | 1.0 ms           | 39.3 ms         |

Opening a module and parsing its file lists (once per copy for copy identity, once more for the read) is most of a small tag's time; decompression is small even for the largest tag. (The 0.27 s noted for `search_definitions` in D37 was a debug build.)

Mapping that module (`globals-rtx-1`, 86,930 entries, 178,575 blocks) takes 0.18 ms; parsing its lists 6.7 ms.

### Step 1: after the in-memory caches (same sample, same test, 2026-11-01)

Kept between calls (decisions D39): each module's parsed lists, parsed definitions, and decompressed tags (LRU, `tag_cache_mb`, default 256).

| Call                                               | First   | Repeated (mean of 2) | Repeated before |
| -------------------------------------------------- | ------- | -------------------- | --------------- |
| `read-tag 0x00008593` (battle rifle, whole)        | 12.1 ms | 1.5 ms               | 20.3 ms         |
| `read-tag 0x00008593 --path barrels[0] --depth 1`  | 0.82 ms | 0.57 ms              | 19.5 ms         |
| `read-tag 0x000041B7 --path barrels[0] --depth 1`  | 0.64 ms | 0.31 ms              | 10.3 ms         |
| `outline --tag 0x00008593 --depth 1`               | 0.57 ms | 0.49 ms              | 19.5 ms         |
| `get-definition weap --path barrels`               | 0.18 ms | 0.15 ms              | 1.7 ms          |
| `search-definitions energy --class weap`           | 0.32 ms | 0.35 ms              | 1.8 ms          |
| `search-definitions rounds` (every class)          | 94.3 ms | 22.4 ms              | 79.6 ms         |

The first call of a session parses the module's lists once (the 12 ms); later calls cost the walk over the tag (`read_tag` walk: 120 ms for the largest tag, 0.2 ms for the battle rifle) and the search over every definition's text.

### Step 2: shorter results (2026-11-01, Forge sample; tokens ≈ characters ÷ 4)

| Call                                                                   | Before                    | After                     |
| ---------------------------------------------------------------------- | ------------------------- | ------------------------- |
| `search-definitions age --class weap`: battery fields in the first 15 rows | 0 of 15 (all "damage" and "message" hits) | 12 of 15 |
| `outline --tag 0x00008593` (default depth 3 → 2, index-only labels left out) | 185 lines, ≈ 3,055 tokens | 164 lines, ≈ 2,585 tokens |
| `outline --tag 0x00008593 --depth 1`                                   | 70 lines, ≈ 1,026 tokens  | 70 lines, ≈ 1,018 tokens  |
| `read-tag 0x00003E1B --path "system.property bindings[*]" --depth 0 --detail brief` | ≈ 1,138 tokens | ≈ 796 tokens   |
| `read-tag 0x0000B130 --path "system.property bindings[*]" --depth 0 --detail brief` | ≈ 267 tokens   | ≈ 205 tokens   |
| `read-tag 0x00008593 --path barrels[0] --depth 1 --detail brief`       | ≈ 1,350 tokens            | ≈ 1,098 tokens            |
| `read-tag 0x00008593 --detail brief` (whole tag)                       | ≈ 6,914 tokens            | ≈ 6,092 tokens            |

"Before" for the brief reads is the new output with each group's prefix put back on its lines and the `in` lines removed (the same text the old renderer printed); it reproduces the simulation's measured 1,138 and 267 exactly.

### Step 3: the size guard (2026-11-01, Forge sample, default limit 3,000 tokens)

| Call                                       | Before                          | After                                          |
| ------------------------------------------ | ------------------------------- | ---------------------------------------------- |
| `read-tag 0x00008593` (battle rifle, whole) | 721 lines, ≈ 8,744 tokens       | 71 lines, ≈ 1,044 tokens (depth-1 outline + hint) |
| `read-tag 0x00008593 --confirm-large`       | —                               | 722 lines, ≈ 8,758 tokens (warning line first)  |
| `get-definition weap` (whole)               | 815 lines, ≈ 8,494 tokens       | 71 lines, ≈ 874 tokens (class outline + hint)   |
| `read-tag 0x000041B7 --path barrels[0]`     | unchanged (path reads are not guarded) | unchanged                               |

### Step 4: the MCP server (2026-11-01, local config with both builds, debug build)

End-to-end over stdio (JSON-RPC lines into `h5tags-mcp --config h5tags.toml`): `initialize` returns the §9a instructions and `serverInfo` `h5tags-mcp 0.1.0`, and the server logged `elicitation=true` for a probe client declaring it; `tools/list` lists the 11 tools; `read_tag 0x00008593 path=magazines[0].rounds loaded maximum` returns `36`; a whole `read_tag 0x00008593` returns the guarded outline; `resources/read h5tags://guide` returns the guide as `text/markdown`; `paths=` returns `isError` with the valid names. Session ready 0.53 s after start (both indexes from cache).

| What the agent receives once per session | Size                        |
| ---------------------------------------- | --------------------------- |
| `initialize` result (with instructions)  | 1,562 characters            |
| `tools/list` result (11 tools, schemas)  | 15,169 characters, ≈ 3,792 tokens |

### Step 5: "What a change affects" in the guide (2026-11-01)

AGENT_GUIDE §2.1: a seven-step workflow (compare with a tag that already behaves as wanted; check on a third tag; follow references out; find references in; search definitions for the behaviour's words; separate tag settings from engine behaviour; answer with a change list), with the battery comparison as its example, plus one rule for tools named in the guide but not built yet. The guide went from 26,681 to 29,546 bytes (275 lines, ≈ 7,386 tokens when read whole as the `h5tags://guide` resource).

### Step 6: README and `.mcp.json` (2026-11-01)

`README.md` with the nine parts of PRD §14, in order. Measured for it: release binaries `h5tags` 4,495,360 bytes (4.5 MB) and `h5tags-mcp` 6,919,680 bytes (6.9 MB), Windows x64. `.mcp.json` registers the development server for this project (`cargo run -q --bin h5tags-mcp -- --config h5tags.toml`). Found while writing it: `guid_check` is accepted in the config but not read; the GUID check always behaves as `warn` (the README says so).

### `guid_check` wired up (2026-11-01, Forge sample; decisions D43)

Weapon `0x00406299` (the first `weap` with a GUID mismatch): `barrels[0].accuracy penalties.single wield penalties` and `…dual wield penalties`. `warn`: both headings end in `warning: definition GUID mismatch`. `off`: no warning. `enforce`: both show `!! definition GUID mismatch at … (definition 7d45a208-4f19-aa30-f6e6-7f849132455b, tag 7db0bb50-4ab6-dd6d-2809-68b1c8e10a13; guid_check = enforce)` and no values. One `srcguid` for both structs, as M3 found for `weap` ("GUIDs" in format-notes).

### Step 7: acceptance session (2026-11-01, fresh Claude Code in an empty folder, release `h5tags-mcp`, local config with both builds)

| Question | Calls (h5tags) | Context before → after (tokens) | Answer |
| -------- | -------------- | ------------------------------- | ------ |
| "Find every tag related to the assault rifle." | 4 `search_tags` (two with `limit=200`), 3 `browse` | 35,981 → 65,162 | ≈ 300 tags by class, build and copies named, inference marked |
| "What's in `objects\weapons\rifle\`?" | 1 `browse` | 65,181 → 66,677 | 34 subfolders with counts; faction grouping marked as its own inference |
| "Battle rifle's first barrel firing settings, every trigger mode" | 1 `browse` (twice), `outline` depth 2, 2 `read_tag path=` | 66,711 → 77,137 | 30 rounds/s, 3 per fire, 0.3 s recovery, `latch-zoom` + all 10 behaviours; copy named |
| "AR vs SMG barrels" | 2 `browse`, 8 `read_tag path=` (brief) | 77,162 → 93,338 | 10 vs 15 rounds/s, accuracy, recoil, effects; tags' units questioned (radians) |

Context = input + cache-read + cache-write tokens from each reply's usage record (the transcript's own numbers). The session started at 35,981 (Claude Code's own prompt plus our ≈ 3,800-token tool list). Largest results: `search_tags limit=200` 23,890 characters (≈ 5,972 tokens); `outline` 10,343 (≈ 2,586). Tool times 6–98 ms; connected in 116 ms; protocol `2026-07-28`.

Done-when: four questions answered with correct values (battle rifle against M3's reads; AR 9.999999 and SMG 14.999999 rounds per second re-read with the CLI); no whole large tag read (outline, then `path=` only; the size guard never had to act); no `!!` value stated as a fact. Found: (1) the agent never read the `h5tags://guide` resource; (2) the instructions called GUID mismatch warnings tool failures, so the agent withheld values it had (fixed, D44); (3) angle values are radians under a "degrees" label (fixed, D44); (4) the instructions name M5 tools (`diff_tags`, `query_field`): the agent looked for them, found none, said so and read by hand (one wasted call). Claude Code's MCP log doesn't keep the server's stderr, so its elicitation support stays unknown.

### Angle units survey (2026-11-01; `h5-meta` `real_tags` `angle_units_survey`)

Forge sample (62,976 tags, 8.8 s) and Xbox sample (57,537 tags, 9.4 s); every non-zero value of the angle types:

| Type [unit] (Forge)                       | Non-zero | Whole degrees if radians | Whole as stored | Largest |
| ----------------------------------------- | -------- | ------------------------ | --------------- | ------- |
| `_field_angle [degrees]`                  | 5,843    | 5,351                    | 34              | 6.283185 (2π) |
| `_field_angle_bounds [degrees]`           | 3,053    | 2,755                    | 0               | 24.434608 (a flinch magnitude) |
| `_field_angle [degrees [0, 180]]`         | 2        | 2                        | 0               | 3.1415925 (π) |
| `_field_real_euler_angles_3d [no unit]`   | 2,181    | 1,867                    | 5               | 6.283185 (2π) |
| `_field_real [degrees]` (plain reals)     | 14,844   | 6,843                    | 7,903           | 9999 |

Plain reals with a degree unit: radian-like values only at `hlmt` `shadow slope scale bias` (×2 fields, 3,389 each), `licn` `angle fade cutoff` / `range` (29 each), one `vehi` steering field (7). After the fix: assault rifle `barrels[0].projectile error.error angle` reads `0.0026179939..0.04014257 rad  0.15..2.3 degrees (max_error_angle)`; every tag of both samples still renders (`real_read`, Forge 36.2 s, Xbox 32.4 s) and the 5 spot values match.

### Retest of "AR vs SMG barrels" (2026-11-01, second fresh session)

The server was the release build from 22:08, before the angle and GUID fixes (committed 23:01), so angles came without `rad`/degrees and the agent inferred radians again. The retest found three gaps in `detail=brief` (D45), all reproduced with the current code on `read-tag 0x00008595 --path barrels --detail brief --depth 3`: `automatic fire variations` (8 entries) showed only entry 0 with no count; the wield-penalty structs lost their `warning: definition GUID mismatch`; collapsed structs showed only `struct`. After the fix the same read is 115 lines, 7,441 characters (≈ 1,860 tokens), with 16 lines or annotations carrying block counts, warnings and `→ path=`. The agent also noted the penalty curves show as `(curve, 68 bytes)`: mapping functions aren't decoded in v1 (PRD §10 item 9).

### Curve fingerprints (2026-11-01, Forge sample; decisions D46)

`barrels[0].accuracy penalties.single wield penalties.firing penalty function[0].function`: assault rifle `(curve, 68 bytes, #9a7cc512)`, SMG `(curve, 68 bytes, #d7f84014)`, so their firing penalty curves differ (the retest's open question). Every tag still reads and renders: Forge 62,976 tags, all with no problem and no divergence (8.7 s); Xbox 57,537 read, 57,518 clean (9.6 s), the same counts as before; `real_read` passes on both.

### Second retest of "AR vs SMG barrels" (2026-11-01; release built after the brief fix, before the curve fingerprints)

Confirmed fixed by the agent: angle fields marked `rad` ("the degree conversions in brackets are just arithmetic"), `automatic fire variations block[8] showing [0]`, and `→ path=` on collapsed structs. New findings: (1) a direct `read_tag path=barrels[0].accuracy penalties.single wield penalties` didn't show the struct's GUID warning (fixed, D47); (2) the agent treated warned values as unreliable, since the bare warning didn't say the values were read (fixed: the warning explains itself, D47); (3) brief shows no units (by design, PRD §7.6); (4) curves not comparable (fixed by D46 after that build); (5) `diff_tags`/`query_field` not reachable (M5). After D47 the same path read shows, in normal and brief: `warning: definition GUID mismatch at barrels[0].accuracy penalties.single wield penalties (values read; only the struct type ID differs)`.

### Third retest of "AR vs SMG barrels" (2026-11-01; release with D46 and D47)

Confirmed by the agent: it now reports the wield-penalty values ("the mismatch warning explains itself … now I can report them"); the curve fingerprints (`#9a7cc512` vs `#d7f84014`) showed the AR's and SMG's firing penalty curves differ; one `read_tag path=barrels depth=5` per weapon replaced eight reads. Still open: `block[8] showing [0]` gives no next step for the other entries; brief has no units (by design; the `detail` description of `read_tag` now says "no units, options or headings", since the agent reads tool descriptions, not the guide); curves not decoded (PRD §10 item 9); `diff_tags`/`query_field` (M5).

### Block hint (2026-11-01; decisions D48)

`read-tag 0x00008595 --path barrels --depth 3`: brief `automatic fire sound.automatic fire variations  block[8]  showing [0] of 8  → path=barrels[0].automatic fire sound.automatic fire variations[*]`; normal heading `… showing [0] 010_wea_un_assaultrifle_fire_a_01  → path=…automatic fire variations[*]`. Every Forge tag still renders (`real_read`, 11.6 s).

### M4 result: passed (2026-11-01, accepted by the user)

Done-when met: a fresh Claude Code session with only the server's instructions and tools answered the four M4 questions correctly, read no whole large tag, and stated no `!!` value as fact (step 7); the guide-coverage test passes. Three retests of the AR vs SMG question confirmed the fixes that followed (D44–D48): radians shown with degrees, GUID warnings that explain themselves and appear on `path=` reads, brief keeping block counts, warnings and `→ path=`, curve fingerprints, and a next step on partly shown blocks.

## M5a: token budget test, header scan, references (2026-11-02, passed: accepted by the user)

### Step 1: token budget test (Forge sample; `h5tags-core` `real_token_budget`)

The 45 distinct h5tags calls of the M4 acceptance session and its three retests (one transcript), replayed through `Command::from_json` as the MCP server runs them, text format. Current sizes: 128,486 characters in all (≈ 32,121 tokens); largest `search_tags assault_rifle limit=200` 23,877, `outline` of the battle rifle 10,345, the two `barrels depth=5` brief reads 9,305 and 8,978. Budgets = size + 20%, 154,199 in all. The run takes 0.25 s.

### Step 2: Q12 survey and the header scan (2026-11-01)

Survey (`h5tags-core` `real_reference_survey`, first copy of each tag): see format-notes "What references without a dependency point to". Forge: 40,672 references without a dependency but not null, 973 to indexed tags; Xbox 53,876, 3,768. Reading headers only: 1.73 s / 0.55 s; headers + tag data: 4.16 s / 1.11 s (one thread, warm).

Header scan (`h5-module` `real_modules` `header_scan_timings`, release, every tag copy, all cores):

| | Forge sample | Xbox sample |
| --- | --- | --- |
| Cold scan | 1.28 s: 10 modules, 62,976 tag copies, 429,388 non-null references (42,732 without a dependency) | 1.15 s: 9 modules, 57,537 copies, 402,086 references (55,363 without) |
| String tables | 27.4 MB | 31.8 MB |
| Cache file / save | 33.9 MB / 64 ms | 37.7 MB / 72 ms |
| Warm start (load + stamps) | 57 ms | 60 ms |
| Tags not readable / modules failed | 0 / 0 | 0 / 0 |

PRD §6.2's target (< 2 min on a full install) can't be measured here; these samples are 1.1 GB of base module each.

### Step 3a–b: scan in the session, `find_references` (2026-11-02, Forge sample, release CLI, empty cache folder)

| Call | Result |
| --- | --- |
| `find-references 0x00008593` (battle rifle, out; first call) | 87 references, by class `fpal 34, wcfg 32, effe 5, bitm 3, …`; 90 lines, ≈ 2,728 tokens; `header scan built: 62976 tags, 1.23 s`; 4.0 s in all (index and scan built from nothing) |
| `find-references 0x0000B131 --direction in` (its bullet) | `br.weapon` (3 of 3 copies) and `br_tuning.weapon`; 272 ms (scan cached) |
| `find-references 0x00008593 --recursive --class bitm` (depth 3) | 92 rows, ≈ 3,852 tokens, with `via` and `(see above)` |
| `find-references 0x00008593 --recursive --max-depth 2` | 2,894 references (paged, 100 per page) |

Dependencies with no reference field pointing to them: 0 on both samples.

### Step 3c–d: `tag_copies`, `search_values` (2026-11-02, Forge sample, release CLI)

| Call | Result |
| --- | --- |
| `tag-copies 0x00008593` (battle rifle) | 3 copies, `versions: 3` (A, B, C) |
| `tag-copies 0x00008595` (assault rifle) | 2 copies, `versions: 2` |
| `search-values battery_remaining` | 49 tags (HUD screens holding `ammo_or_battery_remaining_decision`, `battery_remaining_expression`); 52 lines, ≈ 1,667 tokens; 331 ms (second call; the first after a rebuild took 2.6 s without scanning, cause not measured). In the M4 simulation the same finding took header dumps worth ≈ 17,700 tokens. |

The token budget test still passes (no replayed call grew).

### M5a done-when: `find_references(in)` against an independent pass (2026-11-02; `h5tags-core` `real_reference_survey` `incoming_references_match_an_independent_pass`)

Instead of 3 tags, every target: an independent pass reads every tag copy's reference fields from its header and tag data (`read_stored_tag`, not the scan) and lists, per target, the tags whose copies point to it; `referrers` (the scan's reverse index, what `find_references(in)` shows) is compared for each target.

| | Forge sample | Xbox sample |
| --- | --- | --- |
| Targets with the same referencing tags | **76,724 of 76,724** | **68,184 of 68,184** |
| Target–tag pairs | 202,569 | 187,941 |
| Independent pass | 9.46 s | 8.52 s |

### M5a acceptance session (2026-11-02; fresh Claude Code in the test folder)

Windows Smart App Control blocked the release `h5tags-mcp.exe` (09:38 build, unsigned; Microsoft documents no per-app exception, and on this PC it can't be turned back on once off), so the session ran the **debug build of 09:14**: it has `find_references`; it lacks `tag_copies`, `search_values` and the largest-first class summary, none of which the question needed.

"Which tags reference the battle rifle's bullet?": 4 calls (`browse` of the projectiles folder, `read_tag br.weapon path=barrels[*].projectile`, `find_references in` for `br_bullet.projectile`, and for `br_bullet_hardlight.projectile`). Answer: `br.weapon` (3 of 3 copies) and `br_tuning.weapon`, the same as the CLI and the independent pass; the hardlight bullet is referenced only by `br_barrel_kinetic.weapon_attachment_barrel`. It said which copy it read, that it didn't read `br_tuning` for the field, and that only the indexed modules were searched. The header scan built on first use in 6.34 s (debug build; release: 1.23 s).

Token budget test: the session's 4 calls added (browse 289, read_tag 253, find_references 217 and 156 characters); 49 calls, 129,401 characters, 155,299 budgeted; passes.

### M5a done-when (PRD §9)
- Token budget test passes: yes (49 calls).
- Header scan builds on both samples: yes (Forge 1.28 s, Xbox 1.15 s, release; target < 2 min on a full install not measurable here).
- `find_references(in)` matches a manual check on 3 tags: done for every target instead (76,724 Forge, 68,184 Xbox, all match).
- A fresh session answers "Which tags reference this projectile?" within its budget: yes (4 calls, ≈ 230 tokens of tool results).
