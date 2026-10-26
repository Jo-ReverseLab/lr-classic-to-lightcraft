# lr2lc

lr2lc converts Adobe Lightroom Classic catalogs (`.lrcat`) into LightCraft
libraries. It reads the catalog read-only, never touches the original photos,
and performs a dry run unless `--apply` is given.

This project is not affiliated with or endorsed by Adobe. "Lightroom" is used
only to describe compatibility.

## Status

Early development. The command-line skeleton exists; conversion is not
implemented yet.

## Usage

```sh
cargo run -p lr2lc -- --help
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
