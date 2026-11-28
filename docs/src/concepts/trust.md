# Trust

<p class="kp-lede">A game project is a folder someone else may have made: downloaded, shared, sent by a friend. Opening it should not mean running whatever it holds. Khora treats a project the way a careful reader treats a parcel from a stranger: it looks at what is inside before using any of it, refuses what does not look right, and runs nothing until you say you trust the sender.</p>

This page explains what trusting a project changes, how every file is checked
before it is read, what happens to a file that is refused, and how a shipped
game makes sure its assets are the ones its maker built. For the editor's side —
the restricted bar, the dialogs, releasing a file — see
[Editor: restricted projects](../reference/editor.md#11--restricted-projects).

---

## Trusted or restricted

Every project is either **trusted** or **restricted**. The choice is yours, made
once per project, the first time you open it from the hub or the editor:

| | Trusted | Restricted |
|---|---|---|
| Read and edit every file | yes | yes |
| Play — the project's scripts run | yes | no |
| Its shaders replace the engine's | yes | no — listed as a warning |
| Build Game — its Rust code compiles and runs | yes | no |
| Size caps | the larger ones | the strict ones |

A restricted project is fully usable for reading and editing: nothing of it
*runs*, and its files are read under tighter limits. A project you create in the
hub is trusted from the start. The decision is kept in `.khora/trust.ron` in your
home folder, keyed by the project's real path, and shared by the hub and the
editor; you can change it later from either.

Games and the sandbox have no such question: they read their own files, which
are trusted.

### Shader overlays

A project can replace one of the engine's shader modules with its own: a
`.wgsl` file under `assets/shaders/` whose path names a module the engine
composes is an **overlay** of it. `shaders/lib/std/camera.wgsl` replaces
`khora::std::camera`, `shaders/pipelines/lit_forward.wgsl` replaces
`khora::pipelines::lit_forward` (the `lib/` folder is left out of the name). Any
other `.wgsl` is not an overlay and is ignored.

An overlay is code the graphics card runs, so it follows the project's trust:

- in a trusted project, it is read through the guard and replaces its module;
  saving it recompiles the pipelines that use it, and deleting it gives the
  engine's module back;
- in a restricted project, it is never used, and each one is listed as a
  *Shader overlay ignored* warning.

When the project's trust changes — trusted, restricted, or another project
opened — every overlay is applied afresh, those already on disk included, not
only the ones saved since: trusting a project brings its shaders in at once, and
restricting it gives the engine's back.

## The guard

Every file a project holds is read through one **guard**, at the project's
trust. It runs four checks, in order; the first that fails refuses the file, and
nothing of it is used.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/trust/guard.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>The guard.</strong> A file asks to be read, at the project's trust: trusted or restricted. Four checks follow, in order.</li>
<li><strong>A path that leaves.</strong> A path climbing out with <code>..</code>, an absolute path, or a link pointing outside the project is refused, whatever the file holds. It is never quarantined: the path itself is wrong.</li>
<li><strong>Too large.</strong> A file over the size cap of its kind is refused before a byte of it is read, and quarantined by its size and date.</li>
<li><strong>Not what it says.</strong> A file whose first bytes are not what its name says, or whose decoder goes past its limits or fails, is quarantined by the hash of its content.</li>
<li><strong>Read.</strong> A file that passes every check loads.</li>
</ol>
</figure>

The size caps depend on the file's kind, which follows its asset kind (see
[Asset extensions](../reference/project-structure.md#asset-extensions)), and on the
trust it is read at. A container's external file (a glTF's `.bin`) is no asset: it
is read under its container's cap.

The project's index reads through the guard too: whether the engine builds it
or the editor rebuilds it after a change, a material or a container is read at
the open project's trust, and a refusal reaches the Problems panel like any
other. Build Game reads every file through the guard as well, its refusals
reported the same way. An external file the guard refuses, because a link
leads out of the project or it is over its container's cap, or that is
missing, is left out of the pack and the build goes on: the build's result
line says how many files it left out and names each, and the glTF that needs
one then fails its own load in the game. A glTF's URI leaving the assets folder,
by `..` or as an absolute path, is listed and refused the same way. An asset the
guard refuses, or an identity registry not read, fails the build, and the result
line says why.

| Kind | Trusted | Restricted, imported |
|---|---|---|
| Texture | 256 MiB | 64 MiB |
| Mesh (glTF, OBJ, FBX) and a glTF's external files | 1 GiB | 256 MiB |
| Sound | 512 MiB | 128 MiB |
| Font, text, script, shader, other | 64 MiB | 16 MiB |

Beyond the file's size, each decoder has limits of its own — the bytes it may
decode to, how many elements it may hold, an image's largest side (16 384 pixels
trusted, 8 192 restricted), a glTF mesh's vertices (2²⁴ trusted, 2²² restricted).
A glTF's buffers and images are files too: each is read through the guard, and a
URI that is absolute, names a scheme other than `data:`, or climbs out of the
assets folder is refused (see [Assets — glTF files](./assets.md#gltf-files)). A
decoder that panics is caught: the file is refused, the engine goes on.

A file you downloaded carries the mark your system puts on downloads (on
Windows, its `Zone.Identifier`). In a trusted project, such a file is read as
**imported**, under the strict caps, until you remove the mark.

### Files of the project root

A project holds files outside `assets/` too. Each is read through a guard over
the **project root**, under a cap of its own kind:

| File | Trusted | Restricted, imported |
|---|---|---|
| `project.json` | 1 MiB | 256 KiB |
| `.khora/asset-registry.ron`, the identity registry | 64 MiB | 16 MiB |
| `.git/HEAD` | 4 KiB | 4 KiB |
| `.gitignore` | 1 MiB | 256 KiB |
| A scene outside `assets/` | as text: 64 MiB | 16 MiB |

- **Before the engine starts.** The editor reads `project.json` before the
  engine starts — the game's name, under the strict caps, and the physics step
  the engine runs at, at the trust you decided of the project last time (the
  strict caps when you never did).
- **After you decide.** The project's other reads — `project.json` and
  `.git/HEAD` when it opens, the identity registry, a scene you open from
  outside `assets/` — are read at the trust you decided, their refusals reported
  like any other. The physics step is read again at that trust, so Play steps as
  the game Build Game makes, which reads it at the same trust.
- **An unread registry.** An identity registry the guard refuses, or one that
  does not parse (a merge left its conflict markers in it), is not read: the
  refusal is reported (one that does not parse is quarantined until the file
  changes), the project still opens, each asset under its path's identity, and
  the editor never writes over the file, which would lose the identities it
  holds. Renaming, moving or trashing an asset, and making a file where no file
  is (a new script, a duplicate, a scene, material or prefab saved to a new
  path), are refused meanwhile, saying why: the identity a rename freezes could
  not be kept, nor could a new file drop the identity a trashed file keeps at
  its path, which it would take once the registry reads, and the asset's
  references would break. Saving over a file that is there still works: it is
  the same asset. Build Game fails, as for any registry not read.
- **The registry follows its file.** The file on disk is the truth, and a pull
  or a merge may change it while the project is open. The editor remembers the
  hash of what it read or wrote, and before it renames, moves or trashes an
  asset, makes a new file, freezes a derived identity, or saves, and whenever a reindex lands, it
  compares that hash with the file: changed and parsing, the file is read again
  before the operation, so a registry mended by hand needs no reopen; changed
  and not parsing, the registry is unread, refused as above, and the operation
  refused; unchanged, it goes on. A save writes only over the file it read, so
  the editor never writes its copy over a change made outside it, and it holds a
  lock beside the file while it does, so two editors open on one project save
  one at a time. The engine
  reads the file afresh at every reindex: both give an asset one identity.
- **A trust change reads again.** Trusting a project open releases what its
  restricted caps refused and reads its identity registry again, so a registry
  refused for its size reads, and renames keep identities again.
- **A scene outside the project** is a file you brought in: it is read as
  imported, under the strict caps.
- **The hub** runs nothing of a project and never trusts one: it reads a
  project's `project.json` and `.gitignore` under the strict caps. Each refusal
  is kept in a list of its own, with the project it belongs to, and shown until
  you dismiss it. A launch's banner is made for that launch alone: the
  refusals still listed of the project it launches, then what the launch said,
  never what an earlier launch said; a banner that takes its place leaves them
  listed, shown one under the other, each dismissed alone. Its **Add Native Code** reads the `.gitignore` before it writes
  anything, and writes the whole scaffold or nothing: a refused `.gitignore`,
  or a write that fails midway, leaves the project as it was.

## The quarantine

A refused file is remembered, so it is not read again at every frame or every
launch: it goes to the **quarantine**. What the quarantine remembers decides when
the file comes out of it.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/trust/quarantine.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>Refused for its bytes.</strong> The file is kept under the hash of its content. It stays refused while it holds those bytes, across runs.</li>
<li><strong>Edited.</strong> The file now holds other bytes. It reads again, and its entry is set aside — no longer shown, but kept.</li>
<li><strong>The old bytes come back.</strong> An undo, a sync, a copy over it: the refused content is back, and the file is refused again.</li>
<li><strong>Refused unread.</strong> A file too large to read has no hash: it is kept under its size and date, and released as soon as it can be read — trimmed, or its project trusted. A signed pack entry over its limit is the exception: it is kept under the hash its manifest names for it.</li>
</ol>
</figure>

A few rules keep the quarantine honest:

- **A path that leaves the project is never quarantined.** It is refused every
  time, whatever it holds; there is no file to remember.
- **Files are known by their real path**, however a project spells it — a
  link or a relative path.
- **Trusting a project releases what its restricted caps refused** — and only
  that project's files. A file refused for what it holds stays refused.
- **The quarantine is shared.** Two editors, or an editor and a game, write the
  same store; each write reads it again first and keeps what the others
  quarantined. It lives in `.khora/quarantine.ron` in your home folder — for a
  shipped game, in the game's own data folder.

You can release a file by hand from the editor's inspector; see
[Editor: restricted projects](../reference/editor.md#11--restricted-projects).

## Where a refusal shows

Refusals reach the **Problems** panel under two sources:

- **Security** — each refused or quarantined file, as an error, with what to do
  about it;
- **Restricted** — what a restricted project does not run (a shader overlay
  ignored), as warnings, cleared once the project is trusted.

Every refusal is also written to the log.

## Signed packs

A shipped game reads its assets from a **pack**. Build Game **signs** the pack:
its manifest — every asset's identity, size and content hash, and the size and
content hash of every file a container reads beside it (a glTF's `.bin`), by
path — is signed with your key, and your public key is stamped into the game's binary. At launch the
game checks the signature, then the pack's index against the manifest, and every
asset it loads against its hash. A pack whose index or manifest changed after
it was built is refused, and the game does not start; an asset whose bytes
changed, compressed or not, is refused, reported with the run's other problems,
and fails its own load; so does a container whose external file changed.

A packed game's scripts are compiled from its pack, each read with the same
checks and limits as any asset: a script whose bytes changed is refused and
never runs, and a script left on the disk beside the pack is not the game's.

Every start of a game checks its pack, whether its `main` calls `run_default`
or boots the engine through `run_winit`: a pack that does not verify, or a pack
beside a binary with no key, stops the start. A game that opens its own assets
calls `open_game_assets`, which runs the same checks; the engine then compiles
the pack's scripts, or indexes loose files and compiles theirs, as it does for
`run_default`.

Your key lives in `.khora/signing/pack.key` in your home folder, never in a
project, and is never replaced once made. A game built without a stamped key
reads loose files, unverified: that is a development build. A binary that
carries a key never opens loose files.

**The limit:** a pack is sealed against changes, not against someone who
rebuilds or re-stamps the game's binary. Signing proves the assets are the ones
the binary was built with; it does not prove who built the binary.

## Under the hood

For contributors: the guard lives in `khora-io/src/trust/guard.rs`, the
quarantine in `trust/quarantine.rs`, the decision store in `trust/store.rs`, and
signing in `asset/signing.rs`.

**What the guard guarantees.** Write *R* for a project's real root and *cap(k, t)*
for the cap of kind *k* at trust *t*. A file *f* read at trust *t* is used only if

1. its real path lies under *R* (a link resolved before the test);
2. its size on disk is at most *cap(kind(f), t)*, and the read stops at
   *cap + 1* bytes, so a file growing during the read is refused too;
3. its first bytes match its kind, and its decoder stays within
   *limits(kind(f), t)*;
4. no quarantine entry holds it: a `Content(h)` entry for its path refuses it
   exactly when its bytes hash to *h*; a `Stamp` entry refuses it until it reads
   whole.

The checks run in this order, so an escaping path is never opened and an
oversized file is never read.

**What the quarantine guarantees.** An entry is keyed by real path. A `Content(h)`
entry is set aside, not removed, when the path reads with another hash, so the
same bytes coming back are refused again; a `Stamp` entry is removed on the
first whole read. Trusting a project removes only the entries its restricted
caps made (`TooLarge`, `OverLimit` at the restricted trust, under that project's
root). A write re-reads the store, applies this run's own changes over it, and
replaces the file atomically; two writers merge, with no lock, so the last of
two changes to the *same* entry wins.

**What signing guarantees.** The signature (Ed25519) covers a domain string, the
BLAKE3 hash of the pack's index and the manifest. Verifying it first means
nothing of an unverified manifest is read; checking every load against the
manifest's hash means no asset differs from what was signed.

**Prior art.** The trusted/restricted split follows VS Code's *Workspace Trust*;
the download mark is Windows' *Mark of the Web*; signing a manifest of content
hashes rather than each file is how package registries and app stores seal a
release.
