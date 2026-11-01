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
- AusarDocs `Module.md`. Verified (test) on the Xbox files: `loadmanifest` is a raw file (flags `0x05`), and its size is `numLoadedTags × 4`. No Forge sample has one (`numLoadedTags` = 0).

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
- **Tag references without a dependency can still name a tag** — contradicts the reading "dependency -1 = null". Verified 2026-11-01 by the M2 test `every_tag_header_parses_and_checks` (field read from the tag data at the listed (data block, offset)). Forge: 888,569 tag references, 502,754 with dependency -1, **42,473 of those (8.4%) have a field global ID other than -1**, and 42,264 of those have a non-empty listed name. Xbox: 2,883 references, 1,329 with dependency -1, 575 of those name a tag, all with a listed name. A reference is null only when its field's global ID is -1. The dependency list is therefore not every tag a tag references (PRD §12 Q12).
- **Tag reference field +0x08 is the listed name's length** — verified (same test): in every tag reference of both builds (888,569 Forge, 2,883 Xbox), the `uint32` at +0x08 equals the byte length of the name at the reference's name offset in the string table (0 when there is none). Not in AusarDocs.
- **A reference with a dependency holds its ID and class** — verified (same test): for every reference with a dependency index, the field's global ID (+0x0C) and class code (+0x18) equal the dependency entry's.
- **String tables are Windows-1252** — verified: one Forge tag's string table is not ASCII, and it is not valid UTF-8; its bytes `0x85`, `0x92`, `0xE9` read as `…`, `’`, `é` in Windows-1252 (e.g. a string_id `fleeing_…!!!!`). Xbox: all ASCII. string_id hashes are over the stored bytes; with that, every string_id resolves.
- **Dependencies point at indexed tags by name** — observed (same test): Forge 193,052 dependencies, 154,237 have a global ID found in the sample modules, 154,227 of those have the same name (listed name = indexed path, with or without the class). The other 10 differ only in form (`/` instead of `\`, or a `__chore\gen__\…{ctrim}` generated name). 12,652 distinct global IDs point at tags not in the sample modules. Xbox: 1,316 dependencies, 32 indexed, all with the same name; 571 distinct IDs not indexed.
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

## Map info files (`.mapinfo`)
- Observed on `ng25r_ss_pistons_remixed.mapinfo` (Forge, 2377 bytes, 2026-10-31). Binary format, not documented in AusarDocs; only its readable strings have been looked at. It contains the map name, its scenario path (`levels\multi\ng25r_ss_pistons_remixed\ng25r_ss_pistons_remixed`), and an ordered list of module paths: the map's **module stack**.
  - Order seen: for each layer `-rtx-1`, then `-rtx-11587-1`, then `-rtx-12719-1`: `any\levels\globals-…`, `%(Platform)\levels\globals-…`, `any\levels\multi\<map>\<map>-…`, `%(Platform)\levels\multi\<map>\<map>-…`.
  - `%(Platform)` is a placeholder for the platform folder of the deploy folder.
  - Unverified guess: the `-11587-1` and `-12719-1` modules are update layers over the base `-1` modules, and later layers win when copies differ (PRD §12 Q5).
- The `.mapinfo` is **not** the load manifest. The `loadmanifest` is a file inside the map's own module (AusarDocs `Module.md`).
