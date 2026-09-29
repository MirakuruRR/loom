# Contributing

Thank you for your interest in contributing!

## How this repository works

This repository is a **public mirror**. Day-to-day development happens in a private repository, and each release of Loom is published here.

This has a few consequences for contributors:

- **Open pull requests here, against this public mirror.** This is the right place for contributions, even though it isn't where development happens.
- **Your PR won't be merged through GitHub.** Once accepted, a maintainer applies your changes to the private repository. Your PR will then be closed rather than merged, which is expected and doesn't mean it was rejected.
- **Your changes will appear in the next release.** They'll show up here when that version is published, so there may be a delay between your PR being accepted and your changes appearing on the default branch.

## Before you open a pull request

- Make sure the tests pass with `cargo test`.
- Don't change the version in `Cargo.toml`, and don't add to `CHANGELOG.md`.

## License

Loom is under the zlib license, in [LICENSE.txt](LICENSE.txt). By contributing, you agree that your contributions are under it too.
