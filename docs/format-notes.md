# Format notes

Where each piece of format knowledge comes from. Status: **verified** (checked on real files, named below), **doc only** (from AusarDocs or H5Tags, not yet checked), **contradicts doc** (our files disagree with the source).

Test files (not committed):
- Forge PC: `globals-rtx-1.module` (v27, 86930 files, 24596 resources, 1 GB), `globals-rtx-11587-1.module`, `globals-rtx-12719-1.module`, `ng25r_ss_pistons_remixed.mapinfo`
- Xbox One Guardians: `w1_meridian-rtx-76333-5.module`, `w1_unconfirmed_reports-rtx-81409-2.module`

Checks so far were made by hand with `od` hex dumps (2026-10-31). They will be repeated by the integration tests.

## Modules

### Header
- **Magic** — verified, all 4 files. Bytes on disk are `6d 6f 68 64` (`mohd`). Read as a little-endian `u32` this is `0x64686F6D`, the four-character code `'dhom'` (AusarDocs `Module.md`). Compare it as a `u32`, not as a byte string.
- **Version at +0x04** — verified. **27** in both Forge files, **23** in both Xbox files.
- **v27 header layout (0x38 bytes)** — AusarDocs `Module.md`, "File Header Structure". Verified so far: `numFiles` +0x10, `numLoadedTags` +0x14, `firstResourceIndex` +0x18, `fileNamesSize` +0x1C, `numResources` +0x20, `numFileBlocks` +0x24. `firstResourceIndex = numFiles − numResources` holds in both Forge files (267 − 5 = 262, 24 − 0 = 24).
- **v23 header layout (0x30 bytes)** — verified, both Xbox files. Fields +0x00 to +0x2F have the same layout as v27; there is no `checksum` field. Evidence: the layout computed from these fields ends exactly at the end of each file (see "Data area"), and `firstResourceIndex = numFiles − numResources` (4 − 2 = 2, 61 − 34 = 27).
- **`buildVersionId` (+0x28)** — contradicts doc. AusarDocs says it is always `0xC631DDD9825AF534`. Both Forge files have `0x20A932F3A0855914`. The Xbox files have different values (`0xE64A33B31AFD07B0`, `0x705F93DD3519DAF3`). We don't use this field.

### File entry
- **v27 layout (0x58 bytes)** — AusarDocs `Module.md`, "File Entry Structure". Size checked indirectly: the Forge data area offsets computed with 0x58 point at zlib headers.
- **v23 layout (0x58 bytes, same fields as v27)** — verified on `w1_meridian` (all 4 entries):
  - The file name table starts at 0x190 = 0x30 + 4 × 0x58.
  - Entry 0 (`loadmanifest`): `totalUncompressedSize` 136596 = `numLoadedTags` (34149) × 4.
  - Entries 1–3: `uncompressedHeaderSize + uncompressedTagDataSize + uncompressedResourceDataSize = totalUncompressedSize`. `headerBlockCount + tagDataBlockCount + resourceBlockCount = blockCount`.
  - The resource list `[2, 3]` matches entry 1's `resourceCount` = 2 and entries 2 and 3's `parentFileIndex` = 1.
- **Flags** — verified on `w1_meridian`. `loadmanifest` = `0x05` (Compressed | RawFile, no HasBlocks: one block described by the entry itself). Tag and resource entries = `0x03` (Compressed | HasBlocks).
- **Class code (`groupTag`, +0x40)** — verified on 1 entry (`w1_meridian` entry 1). Stored as a little-endian `u32` four-character code: bytes `70 73 62 73` (`psbs`) = class `sbsp`. Non-tag entries have `-1`.
- **Resource entries** — verified on `w1_meridian` entries 2 and 3: `globalTagId` = -1, `assetId` = -1, `groupTag` = -1, `parentFileIndex` = owning file. So resources are non-tag files under the "global ID ≠ -1" rule.
- **Alignment fields (+0x28..+0x2A)** — verified on `w1_meridian`: they don't add padding to the stored data. Entry 2 has `resourceDataAlignment` = 4 (16 bytes), but its resource section starts at uncompressed offset 0x5B38, which isn't a multiple of 16. Entry 3 has `tagDataAlignment` = 7, and its tag data starts at 0x3E8. In the stored (uncompressed) file, sections are concatenated with no padding. The alignment fields probably apply to runtime buffers.

### File names
- Verified on `w1_meridian` (v23); **not yet checked on Forge**.
  - A **tag's** file name is `tag path` + `.` + `class name`, e.g. `__chore\gen__\levels\campaignworld010\w1_meridian\w1_meridian_elevator_intro{pm}{b2p}.scenario_structure_bsp`.
  - A **resource's** file name is the owner's file name + `[index:type]`, e.g. `...scenario_structure_bsp[0:mesh resource!*]`, `...[1:tag_resources]`. This may answer PRD §12 Q6.
  - Tag paths can contain `{` and `}`.

### Resource list
- Array of `int32` file indices — AusarDocs `Module.md`. Verified on `w1_meridian` (see "File entry").

### Compression block entry
- **v27 layout (0x20 bytes)** — AusarDocs `Module.md`, "Block Definition Structure". Not yet checked.
- **v23 layout (0x14 bytes)** — verified on `w1_meridian` blocks 0–7. Fields: `compressedOffset` u32, `compressedSize` u32, `uncompressedOffset` u32, `uncompressedSize` u32, `compressed` u32 (1 = zlib). No checksum. Each block's compressed offset = previous offset + size, and the uncompressed sizes equal the entry's section sizes.

### Data area
- **Starts right after the block list, no padding** — verified on both Xbox files. `max(dataOffset + totalCompressedSize)` over all file entries, plus the data area start, equals the file size exactly (5657163 and 332888118 bytes). Not yet checked on Forge.

### Compression
- **zlib** — AusarDocs `Module.md`. Verified as far as headers go: every block start checked (all of `w1_meridian`, and the first file of each Forge file) begins `48 0D`. That is a valid zlib header (CM = 8 deflate, CINFO = 4: 4 KiB window, FCHECK valid). Nothing has been decompressed yet.

### Load manifest
- AusarDocs `Module.md`. Verified on `w1_meridian` (v23): file 0 is `loadmanifest`, and its size is `numLoadedTags × 4`. Neither Forge test file has one (`numLoadedTags` = 0).

## Map info files (`.mapinfo`)
- Observed on `ng25r_ss_pistons_remixed.mapinfo` (Forge, 2377 bytes, 2026-10-31). Binary format, not documented in AusarDocs; only its readable strings have been looked at. It contains the map name, its scenario path (`levels\multi\ng25r_ss_pistons_remixed\ng25r_ss_pistons_remixed`), and an ordered list of module paths: the map's **module stack**.
  - Order seen: for each layer `-rtx-1`, then `-rtx-11587-1`, then `-rtx-12719-1`: `any\levels\globals-…`, `%(Platform)\levels\globals-…`, `any\levels\multi\<map>\<map>-…`, `%(Platform)\levels\multi\<map>\<map>-…`.
  - `%(Platform)` is a placeholder for the platform folder of the deploy folder.
  - Unverified guess: the `-11587-1` and `-12719-1` modules are update layers over the base `-1` modules, and later layers win when copies differ (PRD §12 Q5).
- The `.mapinfo` is **not** the load manifest. The `loadmanifest` is a file inside the map's own module (AusarDocs `Module.md`).

## Module header (more)
- The header, file list, file name table, resource list and block list are stored uncompressed. Verified: they were read directly with hex dumps on all test modules. So indexing needs no decompression.
