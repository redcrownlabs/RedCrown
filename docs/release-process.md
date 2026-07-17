# Release Process

Date: 2026-07-17
Status: Accepted pre-release process

## Requirement

A release must be self-contained on Windows, correspond exactly to a reviewed
Git tag, include the Rust backend and media runtime, and provide integrity
hashes. A source archive that cannot start playback is not a RedCrown release.

## Build invariant

`scripts/package-win.ps1` is the single packaging entry point used locally and
by GitHub Actions. It builds the locked Rust release binary, builds the renderer,
stages only the required FFmpeg executables and shared libraries from the pinned
checksum-verified archive, preserves RedCrown and FFmpeg license notices, and
then creates one NSIS installer and one ZIP artifact. It verifies the unpacked
application contains every required runtime before writing `SHA256SUMS.txt`.
Normal artifact compression is intentional: maximum LZMA compression spends
more than 30 CPU-minutes recompressing the FFmpeg DLLs on Windows and makes the
release job operationally fragile without changing application contents.

Electron loads native tools only from `process.resourcesPath` in packaged mode.
Development environment overrides therefore cannot accidentally determine the
contents or runtime behavior of a release.

The renderer build uses relative asset URLs because packaged Electron loads its
entry point through `file://`. Root-relative URLs work on Vite's development
server but resolve against the Windows drive root after packaging, leaving an
empty window. A focused configuration test preserves this requirement.

## Publishing

The release workflow accepts only a `v<package-version>` tag. It repeats the
full format, lint, test, production build, package-content, and checksum gates on
a clean GitHub-hosted Windows runner. The workflow then attaches the installer,
ZIP, and SHA-256 manifest to a GitHub pre-release using the repository-scoped
`GITHUB_TOKEN`.

For version `0.1.0`:

```powershell
git tag -a v0.1.0 -m "RedCrown v0.1.0"
git push origin v0.1.0
```

## Signing tradeoff

The initial pre-release is unsigned because no Windows code-signing identity is
configured. Windows may therefore display an unknown-publisher warning. The
pipeline disables automatic certificate discovery so it cannot silently sign
with a developer-machine certificate. A future signing step must use an
organization-controlled certificate and protected GitHub environment before a
release is promoted from pre-release status.
