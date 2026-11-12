# DanonPDF

DanonPDF will be a free, fast desktop tool for everyday PDF work: merge, split, rotate, reorder and delete pages with thumbnails, compress, and turn images into PDF. Windows first, Linux later. It is part of DanonAA.

> **Status:** early work. The app can merge PDF files, from the window or from the command line. The command line can also count the pages of a file, extract pages and split a file; the window cannot yet. The other tools do not exist.

## Merging PDFs

1. Drag PDF files from the file explorer onto the window, or click **Add PDFs…**. Files that are not PDFs, and files already in the list, are left out, and the line at the bottom says so.
2. Put them in order with **Up** and **Down**, and take out any with **Remove** (the file itself is not touched). **Clear** empties the list.
3. Click **Merge…** and choose where to save. It proposes `merged.pdf` next to the first file, or `merged (2).pdf` and so on when that name is already in the list. It refuses to write over one of the files being merged (a hard link to one of them is not recognised). If you type a name without `.pdf`, it adds `.pdf`, and it does not merge when a file with that name already exists: choose that file in the dialog to replace it. The line at the bottom then says how many pages were written, or why the merge failed.

The merge runs while the window waits, so with very big files the window may stop answering for a few seconds. What a merged file does not keep is listed under [Limits](#limits).

## Command line

`danonpdf-cli` does the work without a window, so a script or an AI agent can use it. It never asks a question, never waits for input and never uses the network.

```sh
danonpdf-cli merge --output merged.pdf first.pdf second.pdf
danonpdf-cli info report.pdf
danonpdf-cli extract --pages 1-3,7,10- --output part.pdf report.pdf
danonpdf-cli split --every 10 --output-dir parts report.pdf
```

| Command | What it does | What it prints |
| --- | --- | --- |
| `merge` | Joins the files, in the order given, into one new file. | `Merged 2 files (12 pages) into merged.pdf` |
| `info` | Counts the pages of a file. | `report.pdf has 12 pages` |
| `extract` | Copies the chosen pages, in the order given, into one new file. | `Extracted 7 pages into part.pdf` |
| `split` | Writes the pages into files of `--every` pages each (one page each without it). | `Split 12 pages into 2 files: parts/report-001.pdf to parts/report-002.pdf` |

When a command fails, it prints `error: …` on stderr instead. With `--json`, every command prints exactly one JSON object on stdout, for a success and for a failure alike, and nothing on stderr:

```text
{"ok":true,"operation":"merge","output":"merged.pdf","inputs":2,"pages":12}
{"ok":true,"operation":"info","input":"report.pdf","pages":12}
{"ok":true,"operation":"extract","output":"part.pdf","pages":7}
{"ok":true,"operation":"split","outputs":["parts/report-001.pdf","parts/report-002.pdf"],"files":2,"pages":12}
{"ok":false,"operation":"merge","error":{"code":"output_exists","message":"…","path":"merged.pdf"}}
```

| Exit code | Meaning |
| --- | --- |
| `0` | The operation succeeded. |
| `1` | The operation was refused or failed. |
| `2` | The command line was wrong. The message is plain text on stderr, also with `--json`. |

### Choosing pages

`--pages` takes a list of items separated by commas. Pages are counted from 1, and they are written in the order given.

| Item | Meaning |
| --- | --- |
| `7` | Page 7. |
| `1-3` | Pages 1 to 3. |
| `10-` | Page 10 and every page after it. |

So `1-3,7,10-` on a 12-page file takes pages 1, 2, 3, 7, 10, 11 and 12, and `3,1` puts page 3 before page 1. A list that cannot be read (`5-3`, `1,,3`, page 0, the same page twice) is a wrong command line: exit code `2`. A page past the end of the file is the failure `page_out_of_range`: exit code `1`. `info` tells how many pages there are.

### What it does and does not do

- It reads every input as given. No file is skipped for its name, and `merge` reads the same file twice when it is given twice.
- It uses output paths exactly as given: it does not add `.pdf`.
- It never writes over an input, not even with `--force` (a hard link to one is not recognised).
- It replaces an existing output only with `--force`.
- `split` names its files after the input, numbered from 1: `report.pdf` gives `report-001.pdf`, `report-002.pdf`… inside `--output-dir`, which it creates if it is missing. It checks every name before it writes anything: if one is taken and `--force` was not given, it writes nothing. If a file cannot be saved halfway through, it stops and keeps the files already written; with `--json`, the error lists them under `written`.

`danonpdf-cli --help` shows all of this with the list of error codes. [AGENTS.md](AGENTS.md) is the same contract written for an agent.

To try it from the build container, put the PDFs somewhere inside the checkout (the container only sees the checkout, mounted at `/workspace`) and pass paths relative to the repository root:

```sh
docker compose run --rm build cargo run --release -p danonpdf-cli -- merge --output merged.pdf first.pdf second.pdf
```

## Limits

- **Only pages travel.** A merged, extracted or split file holds the pages and what they use. Bookmarks, named destinations, form fields, the tagged structure and the document information (title, author) of the inputs are not carried over yet.
- **Links to pages left out stop working.** When pages are extracted or split, a link to a page that is not in the new file stays on its page and leads nowhere. That page is not carried along in secret.
- **A split file can be bigger than its share.** Each new file holds everything its pages point at, used or not. Some programs give every page the same list of all the fonts and images of the document; each file split from such a PDF carries all of them.
- **Splitting into many files takes seconds, not milliseconds.** Each file is made safe on the disk before the next one starts, so that a power cut cannot leave a broken file under a finished name. That wait is paid once per file: 300 one-page files took about 5 seconds in the build container, nearly all of it waiting for the disk.
- **Password-protected files are refused.** A file that opens without a password is read like any other.

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

The last command cross-compiles both Windows programs and leaves them in `dist/`: `danonpdf.exe`, the desktop app, and `danonpdf-cli.exe`, the command-line tool. Run them on Windows: a window does not open from inside a container.

Downloaded crates and build output are kept in Docker volumes, not in the checkout. On Linux, build the image with your own user so the files the container writes stay yours:

```sh
HOST_UID=$(id -u) HOST_GID=$(id -g) docker compose build
```

A named volume keeps the owner it had the first time it was used. If you change `HOST_UID` or `HOST_GID` later, remove the volumes once so they are created again with the new owner. This only deletes the downloaded crates and the build output, which the next build fetches and rebuilds:

```sh
docker compose down --volumes
```

## Try merging from the command line

The core also comes with a small example program that merges PDF files. It shows how to call the core from Rust; to merge files for real, use `danonpdf-cli` (see [Command line](#command-line)). Put the PDFs somewhere inside the checkout (the container only sees the checkout, mounted at `/workspace`), then pass paths relative to the repository root, output first:

```sh
docker compose run --rm build cargo run --release --example merge -- merged.pdf first.pdf second.pdf
```

It prints how many pages it wrote, or why it could not merge.

## Layout

| Path | What it is |
| --- | --- |
| `crates/danonpdf-core` | The PDF logic, with no user interface. |
| `crates/danonpdf-core/examples/merge.rs` | The command-line merge example above. |
| `crates/danonpdf-core/src/pages.rs` | The page syntax (`1-3,7,10-`), shared by every front end. |
| `crates/danonpdf-cli` | The command-line tool (`danonpdf-cli`): arguments in, a line or a JSON object out. |
| `crates/danonpdf-cli/tests/cli.rs` | Runs the real program and checks its output and exit codes. |
| `crates/danonpdf-desktop` | The desktop app (`danonpdf`), built with [egui](https://github.com/emilk/egui). |
| `crates/danonpdf-desktop/src/merge_list.rs` | The merge window's state and rules, tested without a window. |
| `crates/danonpdf-desktop/src/app.rs` | Draws the window and turns clicks and dropped files into calls on that state. |
| `crates/danonpdf-desktop/src/texts.rs` | Every text the user sees, ready for a translation. |
| `scripts/build-windows.sh` | Builds `dist/danonpdf.exe` and `dist/danonpdf-cli.exe`. |
| `AGENTS.md` | How an AI agent uses `danonpdf-cli` and works on this repository. |

## License

[GPL-3.0](LICENSE).
