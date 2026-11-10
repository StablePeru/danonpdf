# DanonPDF

DanonPDF will be a free, fast desktop tool for everyday PDF work: merge, split, rotate, reorder and delete pages with thumbnails, compress, and turn images into PDF. Windows first, Linux later. It is part of DanonAA.

> **Status:** early work. The core can merge PDF files, but the app has no button for it yet: it opens an empty window with its name and version. Merging can be tried from the command line (see below). The other tools do not exist yet.

## Why local

Many people upload contracts, payslips, invoices and scans of their ID to unknown "free PDF" websites, with no idea what happens to those files afterwards. DanonPDF does the same jobs on your own computer:

- no network connections;
- no telemetry;
- no accounts and no ads.

## Build and test

Everything runs in Docker, so the only thing you need installed is Docker with Compose. From the repository root:

```sh
docker compose build
docker compose run --rm build cargo fmt --all --check
docker compose run --rm build cargo clippy --workspace --all-targets -- -D warnings
docker compose run --rm build cargo test --workspace
docker compose run --rm build bash scripts/build-windows.sh
```

The last command cross-compiles the Windows app and leaves it at `dist/danonpdf.exe`. Run that file on Windows: a window does not open from inside a container.

Downloaded crates and build output are kept in Docker volumes, not in the checkout. On Linux, build the image with your own user so the files the container writes stay yours:

```sh
HOST_UID=$(id -u) HOST_GID=$(id -g) docker compose build
```

## Try merging from the command line

The core comes with a small example program that merges PDF files. Put the PDFs somewhere inside the checkout (the container only sees the checkout, mounted at `/workspace`), then pass paths relative to the repository root, output first:

```sh
docker compose run --rm build cargo run --release --example merge -- merged.pdf first.pdf second.pdf
```

It prints how many pages it wrote, or why it could not merge. Bookmarks, named destinations, form fields and the tagged structure of the inputs are not carried over yet.

## Layout

| Path | What it is |
| --- | --- |
| `crates/danonpdf-core` | The PDF logic, with no user interface. All the tests live here. |
| `crates/danonpdf-core/examples/merge.rs` | The command-line merge example above. |
| `crates/danonpdf-desktop` | The desktop app (`danonpdf`), built with [egui](https://github.com/emilk/egui). It only draws and calls the core. |
| `scripts/build-windows.sh` | Builds `dist/danonpdf.exe`. |

## License

[GPL-3.0](LICENSE).
