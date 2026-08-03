# MEGA Git-Xet CLI

`git-xet` is MEGA's bidirectional Git LFS custom transfer agent. It uploads with
the `xet` capability and downloads with the explicit `xet-download` capability,
so large files move directly between the client and MEGA's Xet CAS.

See the [project README](../README.md) for the architecture and transfer
contract.

## Requirements

- Git
- Git LFS
- Linux or macOS on x86-64 or ARM64, or Windows on x86-64 or ARM64

## Install

Linux and macOS:

```sh
curl --proto '=https' --tlsv1.2 -sSfL \
  https://mega.tensorplay.cn/git-xet/install.sh | sh
```

The default destination is `${XDG_BIN_HOME:-$HOME/.local/bin}`. Override it
with `GIT_XET_INSTALL_DIR` for a system-wide or custom installation.

Windows packages and manual archives are published in
[MEGA Git-Xet releases](https://github.com/ohtensorplay/xet-core/releases).

Verify both the executable and Git LFS registration:

```sh
git xet --version
git lfs env
```

## Use

Use ordinary Git commands against MEGA's dedicated Git endpoint:

```sh
git clone https://git.tensorplay.cn/OWNER/REPOSITORY.git
cd REPOSITORY
git lfs track "*.safetensors"
git add .gitattributes model.safetensors
git commit -m "Add model weights"
git push
```

Downloads need no separate wrapper:

```sh
git pull
# or
git lfs pull
```

The repository server chooses `xet-download` only for Xet-native objects and
clients that advertise download support. Standard Git LFS `basic` transfer
remains the fallback.

## Authentication

`git-xet` reuses the credentials associated with the Git remote. A MEGA token
can also be provided through `MEGA_TOKEN` for headless operation.

## Uninstall

```sh
git xet uninstall
```

Then remove the executable with the same package manager or installation method
used to install it.

## Develop

```sh
cargo build -p git_xet
cargo test -p git_xet --lib
cargo check -p git_xet
```
