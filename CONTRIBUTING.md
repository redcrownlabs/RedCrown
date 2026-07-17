# Contributing

RedCrown is still establishing its public contribution process. Small,
well-scoped fixes and tests are welcome.

## Before opening a pull request

1. Open an issue for architectural changes or substantial new behavior.
2. Keep sensitive data, real user libraries, credentials, and copyrighted
   media fixtures out of commits and test output.
3. Add focused tests for behavior changes and document non-trivial design
   decisions under `docs/`.
4. Run the full local check set:

   ```powershell
   npm ci --prefix ./apps/desktop
   ./scripts/tasks.ps1 fmt
   ./scripts/tasks.ps1 lint
   ./scripts/tasks.ps1 test
   ./scripts/tasks.ps1 build
   ```

Pull requests should explain the user-visible reason for the change, the
invariant being preserved, and any tradeoff introduced. Do not submit generated
build output, runtime cache data, endpoint credentials, or real watch history.

## Licensing

By contributing, you agree that your contribution is licensed under the
repository's MIT License. Changes to vendored dependencies must preserve their
upstream license and update the corresponding `VENDORED.md` provenance note.
