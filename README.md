# DanonPDF

DanonPDF will be a free, fast desktop tool for everyday PDF work: merge, split, rotate, reorder and delete pages with thumbnails, compress, and turn images into PDF. Windows first, Linux later. It is part of DanonAA.

> **Status:** early work. The app can merge PDF files. The other tools do not exist yet.

## Merging PDFs

1. Drag PDF files from the file explorer onto the window, or click **Add PDFs…**. Files that are not PDFs, and files already in the list, are left out, and the line at the bottom says so.
2. Put them in order with **Up** and **Down**, and take out any with **Remove** (the file itself is not touched). **Clear** empties the list.
3. Click **Merge…** and choose where to save. It proposes `merged.pdf` next to the first file, or `merged (2).pdf` and so on when that name is already in the list. It refuses to write over one of the files being merged (a hard link to one of them is not recognised). If you type a name without `.pdf`, it adds `.pdf`, and it does not merge when a file with that name already exists: choose that file in the dialog to replace it. The line at the bottom then says how many pages were written, or why the merge failed.

The merge runs while the window waits, so with very big files the window may stop answering for a few seconds. Bookmarks, named destinations, form fields and the tagged structure of the inputs are not carried over yet.

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

A named volume keeps the owner it had the first time it was used. If you change `HOST_UID` or `HOST_GID` later, remove the volumes once so they are created again with the new owner. This only deletes the downloaded crates and the build output, which the next build fetches and rebuilds:

```sh
docker compose down --volumes
```

## Try merging from the command line

The core comes with a small example program that merges PDF files. Put the PDFs somewhere inside the checkout (the container only sees the checkout, mounted at `/workspace`), then pass paths relative to the repository root, output first:

```sh
docker compose run --rm build cargo run --release --example merge -- merged.pdf first.pdf second.pdf
```

It prints how many pages it wrote, or why it could not merge.

## Layout

| Path | What it is |
| --- | --- |
| `crates/danonpdf-core` | The PDF logic, with no user interface. |
| `crates/danonpdf-core/examples/merge.rs` | The command-line merge example above. |
| `crates/danonpdf-desktop` | The desktop app (`danonpdf`), built with [egui](https://github.com/emilk/egui). |
| `crates/danonpdf-desktop/src/merge_list.rs` | The merge window's state and rules, tested without a window. |
| `crates/danonpdf-desktop/src/app.rs` | Draws the window and turns clicks and dropped files into calls on that state. |
| `crates/danonpdf-desktop/src/texts.rs` | Every text the user sees, ready for a translation. |
| `scripts/build-windows.sh` | Builds `dist/danonpdf.exe`. |

## License

[GPL-3.0](LICENSE).
