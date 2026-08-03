# MEGA Git-Xet

<p align="center">
  <a href="https://github.com/ohtensorplay/xet-core/blob/main/LICENSE"><img alt="Apache-2.0 license" src="https://img.shields.io/github/license/ohtensorplay/xet-core.svg?color=2563eb"></a>
  <a href="https://github.com/ohtensorplay/xet-core/releases"><img alt="MEGA Git-Xet release" src="https://img.shields.io/github/v/release/ohtensorplay/xet-core?include_prereleases&label=git-xet"></a>
</p>

MEGA Git-Xet is the native, bidirectional Git LFS transfer agent for
[MEGA](https://mega.tensorplay.cn). It uploads and downloads large model and
dataset files directly between your machine and MEGA's Xet content-addressed
storage, while Git remains the familiar repository workflow.

This distribution is built for MEGA. Its public endpoints, installer, release
artifacts, authentication flow, upload path, and download path are maintained by
the TensorPlay organization.

## Why MEGA Git-Xet

- **Native upload and download.** `git push`, `git pull`, `git fetch`, and
  `git lfs pull` can all use Xet transfers.
- **Direct data plane.** Large file bytes travel directly to or from the MEGA
  Xet CAS instead of being relayed through the Hub web service.
- **Chunk-level deduplication.** Shared binary chunks are transferred and
  stored once, which is especially useful for related checkpoints and dataset
  revisions.
- **Verified downloads.** Every reconstructed file is checked against the Git
  LFS SHA-256 object ID and expected size before it is handed to Git LFS.
- **Scoped access.** The repository server negotiates short-lived CAS access;
  Git credentials remain the source of repository authorization.
- **Graceful compatibility.** Repositories can fall back to the standard Git
  LFS `basic` transfer when the Xet agent is unavailable or an object is not
  Xet-native.

## Install

Install Git and [Git LFS](https://git-lfs.com/) first, then run the MEGA-hosted
installer on Linux or macOS:

```sh
curl --proto '=https' --tlsv1.2 -sSfL \
  https://mega.tensorplay.cn/git-xet/install.sh | sh
```

The installer selects the release for the current OS and architecture, places
`git-xet` on `PATH`, and registers both MEGA transfer names in global Git
configuration.

Verify the installation:

```sh
git xet --version
git lfs env
```

Prebuilt Linux, macOS, and Windows packages are also available from
[MEGA Git-Xet releases](https://github.com/ohtensorplay/xet-core/releases).

## Use it like Git

Clone from the dedicated Git endpoint:

```sh
git clone https://git.tensorplay.cn/OWNER/REPOSITORY.git
cd REPOSITORY
```

Track and upload a large file:

```sh
git lfs track "*.safetensors"
git add .gitattributes model.safetensors
git commit -m "Add model weights"
git push
```

Download tracked files through the normal Git workflow:

```sh
git pull
# or, when the checkout was cloned with LFS smudging disabled:
git lfs pull
```

No MEGA-specific wrapper command is required around Git.

## Transfer contract

| Operation | Negotiated agent | Data path |
| --- | --- | --- |
| Upload | `xet` | client -> `xet.tensorplay.cn` |
| Download | `xet-download` | `xet.tensorplay.cn` -> client |
| Compatibility fallback | `basic` | standard Git LFS object transfer |

The separate `xet-download` capability lets the server select native Xet
downloads only when the installed client actually supports reconstruction.
Older upload-only clients continue to advertise `xet` without being selected
for an unsupported download operation.

The repository service is the control plane: it authenticates the Git LFS batch
request and returns transfer metadata plus short-lived CAS credentials. The
client then exchanges file chunks with the CAS and refreshes access through the
repository service when necessary.

## Authentication

Use the same credentials as the Git remote. For interactive use, sign in with
the MEGA CLI and add the credential to Git:

```sh
mega auth login --add-to-git-credential
```

For headless environments, `MEGA_TOKEN` is supported. Avoid putting tokens in
remote URLs, shell history, repository files, or logs.

## Commands

```text
git xet install [--concurrency N]  Register upload and download agents
git xet uninstall                 Remove the global registration
git xet track <pattern>           Delegate a pattern to git lfs track
git xet transfer                  Git LFS protocol entrypoint
```

Run `git xet <command> --help` for the complete option set. The transfer
entrypoint is normally invoked by Git LFS, not directly by users.

## Build and test

The CLI is implemented in Rust and uses the Xet chunking, reconstruction, and
CAS client crates in this repository.

```sh
cargo build -p git_xet
cargo test -p git_xet --lib
cargo check -p git_xet
```

Key source directories:

- [`git_xet/`](./git_xet): Git LFS agent, installer, and platform packaging
- [`xet_pkg/`](./xet_pkg): high-level upload and download sessions
- [`xet_client/`](./xet_client): CAS and repository HTTP clients
- [`xet_data/`](./xet_data): chunking, deduplication, and reconstruction
- [`xet_core_structures/`](./xet_core_structures): hashes, shards, and Xorb data structures
- [`xet_runtime/`](./xet_runtime): runtime, configuration, logging, and caching

## Contributing

Bug reports and focused changes are welcome in
[ohtensorplay/xet-core](https://github.com/ohtensorplay/xet-core/issues). Please
include the platform, `git xet --version`, `git lfs env` with credentials
redacted, and the failing Git operation.

## License

Apache-2.0. See [LICENSE](./LICENSE).
