# Format notes

Where each piece of format knowledge comes from. Status: **verified** (checked on real files, named below), **doc only** (from AusarDocs or H5Tags, not yet checked), **contradicts doc** (our files disagree with the source).

Test files (not committed):
- Forge PC (v27): `globals-rtx-1.module` (86930 files: 62334 tags, 24596 resources; 1,133,953,321 bytes), `globals-rtx-11587-1.module` (267 files), `globals-rtx-12719-1.module` (24 files), `ng25r_ss_pistons_remixed.mapinfo`
- Xbox One Guardians (v23): `w1_meridian-rtx-76333-5.module`, `w1_unconfirmed_reports-rtx-81409-2.module`, `fo06_deepsea-rtx-94653-1.module`, `fo06_deepsea-rtx-96053-1.module`, `fo06_deepsea-rtx-96490-1.module`, `fo06_deepsea-rtx-97230-1.module` (the last three hold only a `loadmanifest`)

How things were checked:
- **hex**: by hand with `od` hex dumps (2026-10-31).
- **test**: the `h5-module` integration tests (`crates/h5-module/tests/real_modules.rs`): every cross-check, full decompression of every entry, and the format facts report (2026-10-31).

## Modules

### Header
- **Magic** — verified (hex, test), all files. Bytes on disk are `6d 6f 68 64` (`mohd`). Read as a little-endian `u32` this is `0x64686F6D`, the four-character code `'dhom'` (AusarDocs `Module.md`). Compare it as a `u32`, not as a byte string.
- **Version at +0x04** — verified. **27** in every Forge file, **23** in every Xbox file.
- **v27 header layout (0x38 bytes)** — AusarDocs `Module.md`, "File Header Structure". Verified (test) on the 3 Forge files: every field used (`numFiles`, `numLoadedTags`, `firstResourceIndex`, `fileNamesSize`, `numResources`, `numFileBlocks`) yields a layout that passes every cross-check; `firstResourceIndex = numFiles − numResources` holds.
- **v23 header layout (0x30 bytes)** — verified (hex, test), the Xbox files. Fields +0x00 to +0x2F have the same layout as v27; there is no `checksum` field.
- **`buildVersionId` (+0x28)** — contradicts doc. AusarDocs says it is always `0xC631DDD9825AF534`. Every Forge file has `0x20A932F3A0855914`. The Xbox files each have a different value. We don't use this field.
- **Lists are stored uncompressed** — verified (hex, test): the header, file list, file name table, resource list and block list are read directly. Indexing needs no decompression.

### File entry
- **Layout (0x58 bytes, same fields in v23 and v27)** — AusarDocs `Module.md`, "File Entry Structure". Verified (test) on all Forge and Xbox files: section sizes add up to `totalUncompressedSize`, section block counts add up to `blockCount`, resources point back to their owner, and every entry decompresses to its stated size. v23 first checked by hex on `w1_meridian`.
- **Flags** — verified (test). Tags and resources: always `0x03` (Compressed | HasBlocks) in every sample file. `loadmanifest`: `0x05` (Compressed | RawFile, no HasBlocks: one block described by the entry itself), Xbox only (no Forge sample has one).
- **Class code (`groupTag`, +0x40)** — verified (hex, test). A little-endian `u32` four-character code: bytes `70 73 62 73` (`psbs`) = class `sbsp`. Non-tag entries have `-1`. Each of the 205 codes seen always comes with the same class name in the file names. Codes include symbols and a trailing space: `ant!`, `jpt!`, `sfx+`, `sgp!`, `snd!`, `hsc*`, `mat `, mixed case (`GobS`, `LigS`, `SDzs`), digits (`prt3`).
- **Resource entries** — verified (test) on all files: no entry with a parent is a tag (`globalTagId` = -1, `groupTag` = -1, `parentFileIndex` = owner). So resources are non-tag files.
- **Alignment fields (+0x28..+0x2A)** — verified (hex) on `w1_meridian`: they don't add padding to the stored data (a resource section with alignment 16 starts at 0x5B38). The test's section-size check confirms no padding on every entry of every sample file. The fields probably apply to runtime buffers.

### Entries not stored here
- Verified (test) on `fo06_deepsea-rtx-94653-1` (v23), entries 47 and 49: two `mesh resource` entries have `totalCompressedSize` = `totalUncompressedSize` = 0, but nonzero section sizes (4,835,320 and 7,266,178 bytes) and a block list that tiles those sections exactly. Their `dataOffset` points at the data of the next entry (48, 50), and entry 49's blocks would run past the end of the file. So their data is **not stored in this module**; the entry only describes it. Where it is stored is unknown (another module, or streamed).
- Rule used: totals 0 + section sizes > 0 = "not stored here". The size shown is the sum of the section sizes; reading it is an error ("not stored in this module"). Not seen in v27 samples. Not in AusarDocs.

### File names
- Verified (test) on every tag and resource of every sample file, v23 and v27:
  - A **tag's** file name is `tag path` + `.` + `class name`, e.g. `__chore\gen__\levels\campaignworld010\w1_meridian\w1_meridian_elevator_intro{pm}{b2p}.scenario_structure_bsp`. No tag lacks the suffix.
  - A **resource's** file name is the owner's file name + `[index:kind]`, e.g. `...scenario_structure_bsp[0:mesh resource!*]`. Every resource name matches.
  - Tag paths can contain `{` and `}`.
- **Resource kinds seen** (raw, with markers): `api resource*`, `coefficientresource*`, `facial animation resource`, `font file resource`, `mesh resource!*`, `regions resource`, `resource data`, `tag_resource`, `tag_resources`. Markers (`*`, `!`) are stripped for display (decisions D12). May answer PRD §12 Q6.

### Resource list
- Array of `int32` file indices — AusarDocs `Module.md`. Verified (test): every item is a valid file index, and each owner's resources point back to it.

### Compression block entry
- **v27 layout (0x20 bytes)** — AusarDocs `Module.md`, "Block Definition Structure". Verified (test) on the Forge files: blocks tile each entry's uncompressed data exactly, stay inside its compressed data, and decompress to their stated sizes.
- **v27 block checksum** — verified (test `block_checksums_match`, 2026-11-01): the stored `checksum` of all 179,161 compression blocks of the Forge samples equals the **first** 64-bit half (`h1`) of MurmurHash3 x64 128, seed 0, of the block's decompressed bytes. AusarDocs' "the upper 64 bits are discarded" means `h2` is dropped. This also proves every block decompresses exactly. v23 blocks have no checksum.
- **MurmurHash3** — own implementation (`h5-module/src/murmur3.rs`) from the published algorithm. x86 32-bit: matches the published reference values and AusarDocs' `"__default__"` → `0x9B555AD2`. x64 128-bit: verified by the block checksums above.
- **v23 layout (0x14 bytes)** — verified (hex on `w1_meridian`, test on all Xbox files). Fields: `compressedOffset` u32, `compressedSize` u32, `uncompressedOffset` u32, `uncompressedSize` u32, `compressed` u32 (1 = zlib). No checksum.

### Data area
- **Starts right after the block list, no padding; the last entry's data ends exactly at the end of the file** — verified (test) on `globals-rtx-11587-1`, `globals-rtx-12719-1` and the complete Xbox files.
- `globals-rtx-1.module` (1,133,953,321 bytes): verified (test). The data area ends exactly at the end of the file, every check passes, and all 86930 entries decompress (2.1 GB).

### Compression
- **zlib** — AusarDocs `Module.md`. Verified (test): every block of every sample file decompresses with zlib to its stated size (Forge: 2.0 GB from `globals-rtx-1` alone). Every stream starts `48 0D` (CM = 8 deflate, CINFO = 4: 4 KiB window).

### Load manifest
- AusarDocs `Module.md`. Verified (test) on the Xbox files: `loadmanifest` is a raw file (flags `0x05`), and its size is `numLoadedTags × 4`. In the first Forge sample no module had one (`numLoadedTags` = 0). Since 2026-11-01 the Forge `mainmenu` modules have one (42,128 loaded tags in the base, 42,187 in each update layer, whose only entry is the manifest), and the same size rule holds (M1 check `loadmanifest_size`).

### Deploy folder layout
- Observed 2026-11-01 on the Forge sample, with the paths each file had in its deploy folder: modules live in `any\…` and in a platform folder (`pc\…`), under `levels\` (`globals`), `levels\multi\<map>\` and `levels\ui\<menu>\`.
- **The same module file name exists under both, with different contents.** `globals-rtx-11587-1.module` is 11.4 MB under `any\levels\` and 34.0 MB under `pc\levels\`. The `pc` base `globals-rtx-1.module` is 10.4 GB against 1.1 GB for `any`. `fo03_space`'s `pc` copies are 56.5 MB and two empty 56-byte layers, against 466 KB and 279 KB under `any`. The `pc` modules hold the platform-specific tags (`__chore\pc__\…{pc}` names, e.g. bitmaps).
- So a module is identified by its **path** in the deploy folder, never by its file name alone (the index already keys modules by relative path, PRD §6.1).
- An update layer can be empty: a 56-byte file is just a v27 module header with zero entries, and it passes every check.

## Tags (the tag file stored in each tag entry)

Checked 2026-11-01 by a research pass over every tag of both sample sets (62,620 Forge tags, 73 Xbox tags): each tag's header section decompressed and read with the AusarDocs `CachedTag.md` layout. Repeated on every tag by the M2 test `every_tag_header_parses_and_checks` (2026-11-01): 0 parse errors, 0 check failures.

- **Magic and version** — verified. Bytes on disk `75 63 73 68` (`ucsh`) = `'hscu'` as a little-endian four-character code (AusarDocs: `'hscu'`). Version **27 in every tag of both builds**, including the Xbox v23 modules (PRD §12 Q1).
- **Header sizes vs the file entry** — verified: the tag header's `headerSize`, `dataSize` and `resourceDataSize` equal the file entry's three section sizes in every tag.
- **Documented sections** — verified: header (0x50), dependency list (0x18 each), data block list (0x10), tag struct list (0x20), data reference list (0x14), tag reference list (0x10), string_id list (0x8), string table, zoneset info, in that order, back to back. In 55,383 Forge tags (88.4%) and 42 Xbox tags they add up exactly to `headerSize`.
- **Trailing data after the zoneset info** — contradicts doc (AusarDocs doesn't list it). In the other 7,237 Forge tags (11.6%) and 31 Xbox tags, the header section continues after the zoneset info. Its size is always the remainder (`headerSize` − documented sections, never negative); no known header field holds its size or count. Classes: Forge `mat ` 2782, `prt3` 2142, `decs` 1819, `paas` 246, `trac` 189, `licn` 29, `mdsv` 16, `ltvl` 13, `sbsp` 1; Xbox `mat ` 20, `sbsp` 11 (M2 test). Content differs by class:
  - Most are whole multiples of 52-byte records: a `u32` 0x15, three small numbers (e.g. 1 1 1, 1 3 1, 3 3 3), zeros, a 1, then `0xFF7FFFFF` and `0x7F7FFFFF` (the largest negative and positive `f32`, an unset min/max range). The record count matches neither the resource nor the custom tag struct count.
  - `paas` (245 tags) and `sbsp` (12, up to 1.27 MB on Xbox) have other, unparsed contents.
  - Meaning unknown: PRD §12 Q11. The `unknown4C` header field is not a marker (tags with and without trailing data take values from 0 to 0x41).
- **Data blocks are all in the tag data section** — verified: all 1,558,134 Forge and 77,236 Xbox data blocks have `section` = 1; none in the header (0) or resource data (2) section. Their `unknown4` field takes the values 0, 1, 2, 3 and 8 (meaning unknown).
- **Zoneset info** — verified: in every tag, the documented layout (0x10 header with count and list offset, 0x28-byte zoneset entries, 8-byte tag entries) fits inside the section. Sizes seen: 56 bytes upward.
- **Zoneset entries list the resources they load** — extends AusarDocs (it marks +0x04 "Padding?" and leaves +0x08 and +0x10 unnamed). Verified 2026-11-01 by the M2 test `every_tag_header_parses_and_checks` (check `zoneset`) on every tag of both samples:
  - +0x04 (`int32`) is 0 in all 77,153 Forge and 85 Xbox zonesets (padding).
  - +0x08 (`int64`) is a **resource count** and +0x10 (`int64`) the **offset**, from the start of the zoneset info, of that many `uint32` **resource indices**: positions in the file entry's resource list.
  - Each index is below the entry's resource count: verified on every tag, so it is a check. Tags without resources list none.
  - **Usually**, but not always, the zonesets of a tag together list every one of its resources. In the first sample this held for every tag (19,495 Forge and 32 Xbox tags with resources). In the larger samples (2026-11-01: 62,976 Forge and 57,537 Xbox tags, 19,694 and 20,780 with resources) it held for all but **31 bitmaps**. All are `{pc}` bitmaps in one Forge update-layer module, each owning one resource that its only zoneset (`__default__`) doesn't list. So it is not a check.
  - Most tags have one zoneset listing all their resources, 0 to n−1. A BSP with two zonesets splits them: on Xbox, `fo06_deepsea_vista`'s `render_geo` zoneset loads resource 0 (`mesh resource`), and `__default__` loads resource 1 (`tag_resources`).
  - Layout: the resource lists follow the entries, then the tag lists follow. The section size is **exactly** 0x10 + 0x28 × zonesets + 4 × resource indices + 8 × tags in 62,620 of 62,620 Forge and 73 of 73 Xbox tags.
- **A resource's name index is its position** — verified (same test): the resource at position k of a tag entry's resource list is named `<owner>[k:<kind>]` for all 24,601 Forge and 43 Xbox tag resources. So the index in a resource name, the position in the resource list and a zoneset's resource index are the same number (PRD §12 Q9).
- **Zoneset names are the tag's own string_ids** — verified (same test): every zoneset name (77,153 Forge, 85 Xbox) and every zoneset tag's zoneset name (327,459 Forge, 1,318 Xbox) is the MurmurHash3 of a string in the tag's string_id list. Every tag has a `__default__` zoneset (`0x9B555AD2`); 15% of Forge tags have more (up to 63).
- **Tag references without a dependency can still name a tag** — contradicts the reading "dependency -1 = null". Verified 2026-11-01 by the M2 test `every_tag_header_parses_and_checks` (field read from the tag data at the listed (data block, offset)). Forge: 888,569 tag references, 502,754 with dependency -1, **42,473 of those (8.4%) have a field global ID other than -1**, and 42,264 of those have a non-empty listed name. Xbox: 2,883 references, 1,329 with dependency -1, 575 of those name a tag, all with a listed name. A reference is null only when its field's global ID is -1. The dependency list is therefore not every tag a tag references (PRD §12 Q12).
- **Tag reference field +0x08 is the listed name's length** — verified (same test): in every tag reference of both builds (888,569 Forge, 2,883 Xbox), the `uint32` at +0x08 equals the byte length of the name at the reference's name offset in the string table (0 when there is none). Not in AusarDocs.
- **A reference with a dependency holds its ID and class** — verified (same test): for every reference with a dependency index, the field's global ID (+0x0C) and class code (+0x18) equal the dependency entry's.
- **String tables are Windows-1252** — verified: one Forge tag's string table is not ASCII, and it is not valid UTF-8; its bytes `0x85`, `0x92`, `0xE9` read as `…`, `’`, `é` in Windows-1252 (e.g. a string_id `fleeing_…!!!!`). Xbox: all ASCII. string_id hashes are over the stored bytes; with that, every string_id resolves.
- **Dependencies point at indexed tags by name** — observed (same test): Forge 193,052 dependencies, 154,237 have a global ID found in the sample modules, 154,227 of those have the same name. The other 10 differ only in form (`/` instead of `\`, or a `__chore\gen__\…{ctrim}` generated name). 12,652 distinct global IDs point at tags not in the sample modules. Xbox: 1,316 dependencies, 32 indexed, all with the same name; 571 distinct IDs not indexed.
- **Listed names have no class suffix** — verified (same test): the listed name equals the indexed tag path **without** `.<class name>` in all 154,227 Forge and 32 Xbox matches; none is listed with the suffix. Every dependency of both samples has a non-empty listed name. So a dependency's display name is always available: the indexed one, or else the listed name + `.` + the class name (PRD §4).
- **`unknown8` (header +0x08)** — observed: the same value in thousands of tags (e.g. one value in 3,278 tags), so probably per class layout, not per tag. Not used.

## Copies (the same entry in several modules)

Checked 2026-11-01 by the M2 test `copy_identity` on both samples: entries of the same kind and name in several modules, each copy compared with the first one by its decompressed bytes (PRD §12 Q10).

- **Few copies in the Forge layers** — observed: 195 tags (181 in 2 modules, 14 in all 3) and 5 resources have copies, out of 62,411 tags. Every copy has the same global ID as the first. The Xbox sample has no copies.
- **Copies almost always differ** — observed: 213 of the 214 copies differ from the first copy. Only one tag copy is identical (`objects\weapons\rifle\covenant_carbine\sights\covenant_carbine_projection_sight.weapon_attachment_sight`, in `globals-rtx-1` and `globals-rtx-12719-1`). This supports the guess that the update layers hold changed tags (Q5).
- **`assetChecksum` matched data equality in every copy** — observed, not proven: equal for the 1 identical copy, different for all 213 differing ones. With a single identical copy, "equal `assetChecksum` ⇒ identical data" is not shown. Copy identity keeps using the data (PRD §6.4.6).
- **The block checksum lists gave the same answer** — verified on the same 214 copies: equal lists for the identical copy, different lists for the others. No identical copy had a different block split. Block checksums are hashes of the stored data (all 179,161 Forge blocks checked, "Modules"), so equal lists mean equal data, barring a hash collision.
- **Where the copies differ** — observed: in no tag copy is the difference only in the resource data section; the header or tag data always differs too.

## Definitions (Lord Zedd's H5Tags)
- **File names are `(code)class name.xml`, with two caveats** — verified (test + `reference/H5Tags/tags`) against the 205 (code, class name) pairs seen in the sample modules:
  - Trailing spaces are trimmed: class `mat ` → `(mat)material.xml`.
  - Characters not allowed in Windows file names are replaced by `_`: class `hsc*` (`lua`) → `(hsc_)lua.xml`. Whether `_fx_` and other codes with `_` are real or replaced is unknown until seen in a module.
  - So definitions are matched by code first, then by class name (PRD §5.3).
- **Files** — verified (test `real_definitions`, 2026-11-01): all 307 files (289 `tags`, 18 `tag_resources`) are UTF-8 XML without a BOM and parse; 39,735 fields in 56 ms. They use **61** distinct field types (the PRD said 59), and only three element kinds: `<plugin>`, `<item>` and `<_field_*>`. The text uses the entities `&amp;`, `&lt;`, `&gt;`; explanation text spans lines. Only two files have non-ASCII text (`°`).

### Names
Verified 2026-11-01 (tests `name::tests` and `real_definitions::names_and_rules_over_every_definition`) over all 68,781 field and option names. The syntax is richer than PRD §5.3 first said, and is read in this order:
1. **`#help`**: everything after the first `#` (9,002 names). Help can contain `:` and `#`.
2. **`{old name}`**: an earlier or alternate name, e.g. `Radius{Offset}`, `Cubemap Bitmap DEPRECATED{Cubemap Bitmap}` (2,759 names; 551 have two, e.g. `editable{only render in editor}{only visible in editor}`). Braces can hold `:` and `|`, and can follow the units (`falloff begin:wus{sphere falloff begin}`), so they come out first.
3. **`|hint`**: an editor colour or style (`|CCBBAA`, `|ABCDCC`, `|pink`, `|overlay`; 73 names), after the name or after the units. Dropped.
4. **`:units`**, optionally starting with **`[range]`** (`[0.1-1]`, `[0,1]`, `[0,+inf]`): 3,140 with units, 329 with a range.
5. **`X&Y`**: a name and a **label**, with no space around `&` (326 names, e.g. `size class&Analytic Lightmap Size`; a marker may come first: `structure lighting_info^&Structure Lights`). A spaced ` & ` is text (`GOAL & RESPAWN ZONE OBJECT BOUNDARY`); a trailing `&` is a marker.
6. **Markers**: the trailing run of `^ * ! ~ &`. Marker characters elsewhere are text (`can you see me???^` → `can you see me???`). One definition has a stray backtick in the run (``name^` ``); it is dropped.
- Counts: hidden (`!`) 4,431, labels entries (`^`) 1,134. No value field has an empty display name.
- **Every class definition starts with `vtable space` + the internal struct** (289 of 289), and there are **529** mapping-function structs (PRD §5.3's number).
- **Sibling names repeat:** 1,774 fields need `#2`, `#3`… to be unique among their siblings (compared case-insensitively, like paths).

### Custom fields (`_field_custom`)
Verified the same way (978 customs). Three roles, not two:
- **Class section label** (93): all uppercase after trimming `$` and spaces (`OBJECT`, `$$$ UNIT $$$`).
- **Group heading** (533; 217 distinct, e.g. `Misc`, `Outlining`, `Effects`, `Unit Camera`): any other name. It heads the fields after it (in `(bipd)biped.xml`, `Outlining` precedes the outline colour fields).
- **Widget** (352): an editor control. Either it has the same raw name as the next field (`cutoff frequency:Hz` before the real field `cutoff frequency:Hz`), or it is a tool button (`import - directory`, `show bitmap`), or it has no name.

### Field types and sizes
- **The field type table** (`h5-defs/src/field_type.rs`) gives each type's size. AusarDocs `CachedTag.md` "Tag Data" gives five: tag block 0x1C, data reference 0x1C, tag reference 0x20, pageable resource 0x10, string_id 4. The other 56 are candidates (natural sizes: `_field_string` 32, `_field_long_string` 256, `rgb_color`/`argb_color` 4, `point_2d` 4, `rectangle_2d` 8, …) **confirmed by the survey** (test `survey`, decisions D31, 2026-11-01):
  - **Main structs:** the definition's top-level size equals the main data block's size in **62,976 of 62,976 Forge tags** (210 classes, every class in the sample).
  - **Tag blocks** whose GUID matches the definition: data block size = entry count (`int32` at +0x10 of the tag block field, PRD §5.2) × the definition's entry size in **790,750 of 790,750** Forge blocks.
  - So every size used in those structs is right. A type that only occurs in structs not covered (classes without sample tags, unmatched blocks) is not proven by this.
- **Arrays are one instance of their children** (PRD §12 Q7) — verified by the same numbers: the 274 array fields have no count attribute, and treating each as one instance gives the exact sizes.
- **Xbox:** 57,518 of 57,537 main structs and 975,247 of 975,249 tag blocks match. The 19 that don't are in 5 UI and globals classes (`cnmp` −24 bytes, `rasg` −32, `sngl` −32, `wgtz` −32, `wigl` −72, plus 2 `wigl` blocks): the definitions describe the Forge layout, and these Xbox tags are smaller. The size check (§6.4.5) catches them.

### GUIDs (PRD §12 Q3)
- **`srcguid` is a Windows GUID** — verified (survey): tag struct GUIDs equal the definition's `srcguid` with the first three groups little-endian (`151d0146-479c-181f-…` is stored `46 01 1d 15 9c 47 1f 18 …`): 1,880,332 matches on Forge, 2,408,432 on Xbox, and **0** in the as-written order.
- Custom structs (kind 3) match a `srcguid` of the class's definition in 5,353 of 5,353 (Forge) and 5,525 of 5,525 (Xbox).
- **Tag blocks (kind 1) match in 99.8%:** 1,874,979 of 1,878,819 Forge. The 3,840 others have GUIDs found in **no** definition: 11 classes, mostly `coll` collision models (3,633), plus `vehi`, `subt`, `cmps`, `comp`, `motl`, `scnr`, `cusc`, `gpso`, `cust`, `lgtd`. Xbox: 2,483, the same classes plus `dctr`. So a GUID mismatch is not proof of a misread: at these fields the tag's struct type differs from what the definition says, and the size check decides (decisions D32).
- **Main structs (kind 0):** each class has exactly **one** main struct GUID across all its tags (210 of 210 Forge classes, 214 of 214 Xbox), and it appears in no definition. It identifies the class's layout. Lead for M6: a class whose main GUID differs between builds probably has a different layout.
- **Resource structs (kind 2):** their GUIDs appear in no definition, `tag_resources` included (25,332 Forge, 27,822 Xbox). So they don't identify the resource layout (PRD §12 Q6 stays open).

## Map info files (`.mapinfo`)
- Observed on `ng25r_ss_pistons_remixed.mapinfo` (Forge, 2377 bytes, 2026-10-31). Binary format, not documented in AusarDocs; only its readable strings have been looked at. It contains the map name, its scenario path (`levels\multi\ng25r_ss_pistons_remixed\ng25r_ss_pistons_remixed`), and an ordered list of module paths: the map's **module stack**.
  - Order seen: for each layer `-rtx-1`, then `-rtx-11587-1`, then `-rtx-12719-1`: `any\levels\globals-…`, `%(Platform)\levels\globals-…`, `any\levels\multi\<map>\<map>-…`, `%(Platform)\levels\multi\<map>\<map>-…`.
  - `%(Platform)` is a placeholder for the platform folder of the deploy folder.
  - Unverified guess: the `-11587-1` and `-12719-1` modules are update layers over the base `-1` modules, and later layers win when copies differ (PRD §12 Q5).
- The `.mapinfo` is **not** the load manifest. The `loadmanifest` is a file inside the map's own module (AusarDocs `Module.md`).
