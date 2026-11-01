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
