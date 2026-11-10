# Media and the blob store: design note

Status: **built** (Phase A: the blob store, Media uploads, serving, thumbnails, quotas,
sweeps, storage settings; Phase B: Artifacts and the agent items they add, marked **[B]**, see
[artifacts.md](artifacts.md); Phase C: images in chats, marked **[C]**). The contract is the
sidebar-areas design (v2), §5 and §6, with its Appendix B; this note says what the code does
and why.

## What Media is

Media is each account's **private library** of images, video and audio.

- An item comes from an upload on the Media page, from an agent's published file **[B]** (one
  item per artifact lineage, showing its newest version), or from an image attached in a chat
  **[C]**.
- **Private to its owner.** Nobody else sees an item, workspace admins and server admins
  included (T10); the API token sees all. A request for someone else's item is answered 404,
  never 403, so ids are not confirmed.
- **The workspace filters, it never shares.** An item remembers the workspace it was added
  in; lists show the items of the workspace the Shell is in. Its *effective* workspace is that
  one while the owner is still a member (or the workspace still exists), else the owner's
  Personal workspace, the same self-healing rule as chats (`MEDIA_WS` in
  `crates/store/src/media.rs`).
- **Media files only.** Documents, archives and binaries are refused with 415 "not a media file;
  build outputs go to Artifacts". A PDF is a document.
- **On by default** **[C]**. `[media] enabled = false` in config.toml turns it off: every
  `/api/media` route then answers 404, `features.media.show` is false, search has no Media
  tab and chats take no images. Agent items are still recorded (they are only references), so
  they appear once Media is on again.

## The blob store

Media and Artifacts share one store (`crates/master/src/storage.rs`, `blobs.rs`; tables in
`crates/store/src/blobs.rs`).

### Files on disk

- `<data>/blobs/<last two hex of blob_id>/<blob_id>.rzb`, directories 0700, files 0600. Sharded
  by the *last* characters, because a uuid v7 starts with the time.
- `<data>/blobs/tmp/` holds files being written.
- **Write order:** stream into `tmp/` while hashing (SHA-256 of the plaintext) and encrypting;
  fsync the file; rename it into its shard; fsync the directory; insert the row in one store
  transaction with the row that references it and the quota check. A crash leaves at most a
  file without a row. Renames and inserts hold one process-wide lock that the sweeps also take,
  so a sweep never deletes a file whose row is about to be written.

### Encryption at rest, deletion by crypto-shredding

- Each blob has its own random 32-byte key, stored only in its `blobs` row in master.db.
- File format: `RZB1`, a 7-byte random nonce prefix, then the plaintext in 64 KiB segments,
  each sealed with AES-256-GCM (aws-lc-rs, already linked for TLS) in the STREAM
  construction: segment `i`'s nonce is the prefix, `i` big-endian and a last-segment flag; the
  AAD is the blob id. Reordered, dropped or swapped segments, a truncated file and a file
  renamed to another blob's id all fail to open.
- Range reads decrypt only the segments they cover; memory stays flat (one segment at a time,
  a few segments ahead of the socket).
- **Deleting** a file is: delete the row (`secure_delete` overwrites it) → `wal_checkpoint
  (TRUNCATE)` → unlink the file.

**What this guarantees:** after a delete, neither the live files under `blobs/` nor the live
master.db and its WAL hold the data or its key; a backup of `blobs/` without master.db is
unreadable. **What it does not:** a checkpoint releases disk blocks without overwriting them,
so SSD wear-levelling or copy-on-write filesystems may still hold old copies of master.db
pages, and with them old keys. Back up `blobs/` and master.db together: neither is useful
without the other.

### Dedupe

Per owner: the same bytes (same SHA-256) stored twice by one account are one blob, so a second
copy costs no quota. Never across accounts, so nobody can learn whether someone else holds a
file. In Media, the same bytes added again in the same effective workspace are the same item
(`200 {…, duplicate: true}`); in another workspace they become a second item on the same blob.

### Quotas and limits (`[storage]` in config.toml)

| key | default | meaning |
|---|---|---|
| `max_file_mb` | 200 | one upload (and one published file or folder pack **[B]**) |
| `account_quota_mb` | 4096 | an account's blobs, Media and Artifacts together; 0 = unlimited; the API token is unlimited |
| `server_quota_mb` | 20480 | every blob together |
| `min_free_mb` | 1024 | free space kept on the data disk |

- The quota is enforced inside the insert transaction (`SUM(size)` of the owner's blobs). A
  dedupe hit costs nothing.
- **Over a limit the file is refused; nothing is ever evicted.** An upload whose
  `Content-Length` does not fit the remaining room gets 507 before any byte is read; without
  a length, 507 as soon as the running count passes it, and 413 past `max_file_mb`.
- The free-disk floor is checked (statvfs) before a write and every 16 MiB during it.
- The settings are read on every request, so an admin's change applies without a restart.

### Upload slots and timeouts

- 2 uploads at once per account and 4 on the server, taken without waiting: when none is
  free the answer is **429 with `Retry-After: 5`**, never a queue.
- 30 s without a body chunk, or 30 min in all → **408**, and the temp file is deleted.

### Sweeps

- **At start** (before any node links): every temp file, and every shard file without a row.
- **Hourly** (the master's one maintenance loop): blob rows nothing references, then temp
  files and row-less shard files older than an hour (younger ones may still be committing),
  then a WAL checkpoint when rows went.
- **After an account is deleted:** its blob, item and preview rows went with the account
  (foreign keys) and its name-index rows in the same transaction (`blobs::on_user_deleted`);
  the master then checkpoints the WAL and unlinks every file without a row.

## Sniffing: what a file is

`crates/master/src/sniff.rs` decides from the bytes, never from the name: `kind` (image,
video, audio, binary, package, archive, document, other), `format` (png, mp4, mov, heic, elf,
tar.gz, …; `bin` if unknown), the type it is served with, and `display`:

| kind | `inline` (its own type) | `img` (its own type only to an `<img>`) | `download` |
|---|---|---|---|
| image | png, jpeg, gif, webp, avif, bmp, ico | svg, heic, heif | tiff, jxl, psd, … |
| video | mp4, webm, ogg (theora), mov (`video/quicktime`) | – | mkv, avi, … |
| audio | mp3, ogg (vorbis, opus), wav, flac, m4a, aac, webm audio | – | midi, aiff, … |
| everything else | – | – | pdf, html, xml, zip, elf, … |

- The `infer` crate reads magic numbers in the first bytes. Written here: SVG (BOM, XML
  prolog, comments and doctype, then `<svg`), the ISO-BMFF brands (HEIC/HEIF, AVIF,
  QuickTime, MP4, M4A), Ogg codecs, the webm/Matroska DocType, and executable targets (ELF,
  PE, Mach-O, wasm: "x86_64 Linux").
- **A declared type or name only narrows within the detected family**: `audio/webm` or
  `.weba` makes a webm audio, `.m4a` an MP4 container audio, `.jar` a zip a package, `.tar.gz` a
  gzip a tarball. It never crosses: an HTML page named `cat.png` is a document (and refused by
  Media); a real PNG with HTML after its header is a PNG.
- **Image dimensions come from headers only** (PNG IHDR, JPEG SOF, GIF, WebP VP8/VP8L/VP8X,
  BMP, ICO, HEIF/AVIF `ispe`), at commit, into `blob_previews`. Nothing is decoded on the
  server. The readers are bounds-checked and fuzzed with mutated and random input.
- The browser never decodes an image above 40 MP or 16,384 px a side: the Media page shows an
  icon and offers Download instead (`web/src/lib/thumbs.ts`).

## Documents attached to a message are not Media (T20)

`POST /api/attachments` (the composer's `+`) keeps no file: it extracts text, stores it in
master.db `attachments` until the message is sent, and the text then lives in the message's
event. The same rule as above decides what a file is: a PDF only by `%PDF-` at offset 0,
never by name. PDFs are read by the confined `rizzy-harness pdf-text` helper
(`crates/master/src/pdf.rs`), never in the server; refusals: 422 (not readable, no text
layer, over 20 s, no Landlock on the server), 503 (two PDFs already being read and no slot
within 10 s), 429 (20 unsent files or 8 MiB of unsent text for this account). Unsent uploads
expire with the incognito TTL.

## Serving stored files (T17)

Every content, download and preview endpoint goes through `blobs::serve`, thumbnails through
`blobs::serve_thumb`, after the endpoint checked access (404 when invisible).

- **Content-Type** only from the sniffed row: `display = inline` on a showing endpoint is
  served as its type; `display = img` only when the request has `Sec-Fetch-Dest: image`;
  everything else, every `?download=1`, and browsers that send no `Sec-Fetch-Dest` get
  `application/octet-stream` with `Content-Disposition: attachment` (fails closed).
- **Headers on every response:** `Content-Security-Policy: default-src 'none'; img-src 'self'
  data:; media-src 'self'; style-src 'unsafe-inline'; sandbox` (the app's guard keeps a
  handler's CSP), `X-Content-Type-Options: nosniff`, `Cross-Origin-Resource-Policy:
  same-origin`, `Cache-Control: private, no-cache`, and `ETag: "<sha256>"` (`If-None-Match` →
  304). Deletes act at once: nothing is cached without revalidation.
- **SVG and HEIC:** `Vary: Sec-Fetch-Dest`, and the `<img>` variant's ETag is
  `"<sha256>-img"`, so a cached image response never revalidates into "open image in new tab"
  as a document.
- **Downloads** also carry `X-Content-SHA256`.
- **Range:** one `bytes=` range → 206 with `Content-Range`; malformed or outside the file →
  416; several ranges → 200 with the whole file.
- **File names** are cleaned to one line (no quotes, separators or controls), sent as an ASCII
  fallback plus `filename*=UTF-8''…`, and get the format's extension appended when their own
  last extension is not one of it (`x.mp3.html` stored as mp3 → `x.mp3.html.mp3`).
- **Never:** import from a URL, serving HTML or script from a blob, decoding, transcoding or
  thumbnailing on the server.

## Thumbnails

The owner's browser makes them (`web/src/lib/thumbs.ts`): images through a canvas, video from
a frame, WebP ≤ 320 px. `PUT /api/media/{id}/thumb` takes WebP, JPEG or PNG by magic bytes
(else 415), at most 64 KiB (else 413), **from the blob's owner only**, so a viewer can never
plant one. A browser that cannot make one (HEIC or MOV it cannot decode) sends `PATCH
{no_thumb: true}` and it is never retried.

## API

`MediaItem`:

```ts
interface MediaItem {
  media_id: string; name: string; kind: "image" | "video" | "audio" | "other"; // other: display = download
  format: string; mime: string; size: number;
  width: number | null; height: number | null; duration_ms: number | null;
  display: "inline" | "img" | "download"; has_thumb: boolean; no_thumb: boolean;
  origin: "upload" | "chat" | "agent";
  source: { session_id: string; title: string } | "deleted" | null;  // only the owner's own chat
  artifact: { artifact_id: string; version_id: string; seq: number; name: string } | null; // [B]
  chats: number;                                                      // [C]
  workspace_id: string | null;   // the effective workspace
  created_ms: number; updated_ms: number;
}
```

| Method and path | Body | Answer / errors |
|---|---|---|
| GET `/api/media?workspace=&kind=&origin=&session=&q=&before=&limit=60` | – | `{items, next, usage:{used, quota}, limits:{max_file}}`; `kind` is `image`, `video`, `audio` or `other`; `next` is the cursor to pass as `before`; `quota` 0 = unlimited |
| POST `/api/media?workspace=&origin=upload\|chat&session=` | the raw file, streamed; name in `X-File-Name` (percent-encoded UTF-8, never in the URL) | 201 `MediaItem` · 200 `{…, duplicate: true}` · 408 · 413 · 415 · 429 · 507; `origin=chat` **[C]**: images only (else 415), `session` the chat when it exists already (someone else's: 404; incognito: 409) |
| GET `/api/media/{id}` | – | `MediaItem` · 404 |
| PATCH `/api/media/{id}` | `{name?, width?, height?, duration_ms?, no_thumb?}` | `MediaItem`; hints clamped to 1..100000 px and ≤ 48 h, width and height ignored for images (header values win); hints and `no_thumb` from the blob's owner only (403) |
| DELETE `/api/media/{id}` · POST `/api/media/delete` `{ids}` (≤ 100) | – | `{ok}`; ids that are not the caller's are skipped silently |
| GET `/api/media/{id}/content[?download=1]` | – | the file (above) |
| GET · PUT `/api/media/{id}/thumb` | PUT: WebP, JPEG or PNG ≤ 64 KiB | image · 404 / `{ok}` · 413 · 415 |
| GET `/api/me/storage` | – | `{used, quota, media:{bytes, items}, artifacts:{bytes, versions}}` |
| GET `/api/admin/storage` | server admin, API token | `{settings, usage:[{user_id, username, used, media_items, artifact_versions}], total, disk:{free, total}}`: numbers only, never file names or contents |
| PUT `/api/admin/storage` | `{max_file_mb, account_quota_mb, server_quota_mb, min_free_mb}` | the settings; 400 on bad values |

Every change sends `media_changed` (content-free) to the owner's sockets only.

**Search:** `media::search` is the search panel's provider over the names (`media_fts`), with
the list's visibility (owner, effective workspace): rows of `type` image, video, audio or
`media_other`, preview "Uploaded" / "From chat ‘…’" / "Published by an agent", and the
thumbnail as a same-origin path. The list's `q` uses the same tokens and the same index.

## Images in chats [C]

The composer uploads each image to Media (`origin=chat`, after the browser downsized and
re-encoded it, which also drops EXIF and GPS), then sends the message with their ids:
`POST /api/sessions/{id}/prompt` `{text, images: [media_id]}`.

**`media::prompt_images` checks, in order** (`crates/master/src/media.rs`):

| check | refusal |
|---|---|
| Media is on | 404 |
| at most 4 images, each once | 413 · 400 |
| the chat is not incognito | 409 "Images can't go into an incognito chat." |
| each is the caller's own item (the API token: anyone's), an image in PNG, JPEG, WebP or GIF (what every vision provider reads) | 404 · 415 |
| at most 5 MiB together | 413 "Too much attached to one message" |
| the chat's node announced the `images` capability | 422 "This chat's worker can't take images yet; update it." |
| the chat's model sees images (`vision` in the model's configuration; unlisted counts as no) | 422 "<model> can't see images; choose a model that can." |
| the chat carries at most 50 images (ones it already carries are free) | 409 |

A turn run by another agent (an @mention) or compared across models is checked again in
`Master::prompt` against each of those models; a refusal there is 409 with the same text,
because the HTTP layer maps that error to 409. An answer to a question never carries images
(400).

**The 8 MiB frame.** `Master::prompt` measures the serialized `session.prompt` (images in
base64, up to five 100,000-character attachments, the agent spec, the context) and refuses
anything over 7 MiB with 413 "Too much attached to one message", before it reaches the link.
Nothing is recorded and the chat carries nothing.

**On the wire:** `session.prompt.images: [{media_id, sha256, mime, name, width, height, data}]`
(`data` base64), only to nodes whose hello listed `images`. `media_id` is there because the
node writes the message: `user_message.images: [{media_id, sha256, mime, name, width,
height}]`, references only, never bytes. Both folds copy them to `UserItem.images`.

**On the node** (`crates/node/src/images.rs`, `session.rs`): the limits again, the type, the
base64 and the sha256 of every image are checked; the bytes go into node.db (or chat.db)
`session_images`, keyed by (chat, sha256), and are deleted with the chat under
`secure_delete`. Incognito chats refuse images here too.

**To the model** (`crates/agent-native/src/images.rs`): a user message with images becomes
`ChatMsg::UserImages`, sent as `ContentPart::Binary` parts after its text. Only the newest
three image-bearing messages are shown again; older ones, and any whose bytes are missing,
name each image as `[image: name]` in the text, so a long chat does not pay for every picture
on every step. Summaries and sub-agents get text only. The mock provider answers "I see N
image(s)." to a message with images.

**Which chats carry an item:** `media_uses(media_id, session_id)` gets a row once the prompt
reached the node; `MediaItem.chats` counts them, and a chat's deletion removes its rows. An
item uploaded before its chat existed (the home composer) takes the first chat it is sent in
as its source. `DELETE /api/sessions/{id}?media=1` first deletes the caller's items whose
source is that chat (sent there, or published there by an agent; the artifacts stay).

`features.media.vision_images` is true while Media is on and some listed model sees images;
the composer still checks the chat's model.

## Deleting, honestly

- Deleting an item deletes its blob when nothing else references it (another workspace's item
  on the same bytes, or an artifact version **[B]**): row, WAL, file, in that order.
- Deleting an agent item never touches the artifact version behind it **[B]**.
- Images sent in chats **[C]**: deleting the Media item does not reach the copies a chat's
  worker node keeps for the conversation (they go when the chat is deleted), and copies
  already sent to a model provider cannot be recalled. `MediaItem.chats` says how many chats
  still carry the item, and the delete dialog says so ("Still in N chats. Delete those chats
  to remove it there. Copies already sent to the model provider can't be recalled.").
