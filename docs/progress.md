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

## M2: tag decompression and tag headers (in progress, 2026-11-01)

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
