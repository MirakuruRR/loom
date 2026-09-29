# Loom

Loom is a language that compiles to Unreal Blueprints. To install and use it, see [loom.pierrekin.me](https://loom.pierrekin.me).

## Building

You need stable Rust. The tests that run in Unreal also need a [supported Unreal](https://loom.pierrekin.me/download/) that can build plugins.

To run the tests that don't need Unreal:

```sh
cargo test
```

To build the examples in Unreal and run their tests:

```sh
cargo xtask plugin
cargo xtask examples
```

Both use the engine `tests/project` names. To use another engine, add `--engine <dir>`, and to build into another project, `--project <dir>`.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

zlib. See [LICENSE.txt](LICENSE.txt). 