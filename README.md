# DanonPDF

DanonPDF will be a free, fast desktop tool for everyday PDF work: merge, split, rotate, reorder and delete pages with thumbnails, compress, and turn images into PDF. Windows first, Linux later. It is part of DanonAA.

> **Status:** early work. The app can merge PDF files, extract pages from one and split one into several files, from the window or from the command line. The other tools do not exist.

The window has two views, **Merge** and **Split**, chosen with the two buttons at the top. It opens on Merge. Files dropped on the window go to the view that is shown, and each view keeps what it holds while the other one is open.

## Merging PDFs

1. Drag PDF files from the file explorer onto the window, or click **Add PDFs…**. Files that are not PDFs, and files already in the list, are left out, and the line at the bottom says so.
2. Put them in order with **Up** and **Down**, and take out any with **Remove** (the file itself is not touched). **Clear** empties the list.
3. Click **Merge…** and choose where to save. It proposes `merged.pdf` next to the first file, or `merged (2).pdf` and so on when that name is already in the list. It refuses to write over one of the files being merged (a hard link to one of them is not recognised). If you type a name without `.pdf`, it adds `.pdf`, and it does not merge when a file with that name already exists: choose that file in the dialog to replace it. The line at the bottom then says how many pages were written, or why the merge failed.

The merge runs while the window waits, so with very big files the window may stop answering for a few seconds. What a merged file does not keep is listed under [Limits](#limits).

## Extracting pages and splitting

Click **Split** at the top of the window.

1. Drag one PDF onto the window, or click **Choose PDF…**. Its name and its number of pages appear in the box.
2. Choose **Extract pages** or **Split into files**, and fill in the one field below.
3. Click **Extract…** or **Split…** and choose where to save. The line at the bottom says what was written, or why nothing was.

| Mode | The field | What the button writes |
| --- | --- | --- |
| **Extract pages** | The pages to take, such as `1-3,7,10-` (see [Choosing pages](#choosing-pages)). The line under it says how many pages that selects, or what is wrong with it. | One new PDF with those pages, in the order typed. It proposes `report-pages.pdf` next to `report.pdf`, or `report-pages (2).pdf` and so on when that name is taken. |
| **Split into files** | How many pages go into each file: 1 gives one file per page. The line under it shows the result, such as `5 pages into 3 files: report-001.pdf to report-003.pdf`. | Those files, into the folder you choose. The last one may have fewer pages. |

What the view does and does not do:

- **One file at a time.** A new file replaces the chosen one; what you typed stays and is checked against the new file. Of several files dropped together, the first PDF is used, and the line at the bottom says how many were left out.
- **Only files it can work on are accepted.** A file that needs a password, is damaged or is not a PDF is not chosen, and the line says why.
- **The chosen file is only read.** Neither button writes over it (a hard link to it is not recognised).
- **Extract** treats the name you choose as Merge does: it adds `.pdf` to a name without it, and then does not write when a file with that name already exists.
- **Split** checks every name before it writes anything. If one already exists in the folder, it writes nothing and names the first one: choose another folder, or an empty one. The window never replaces the files of an earlier split; `danonpdf-cli split --force` does.
- If a split cannot save a file halfway through, it stops, keeps the files already written and says how many there are.

Both run while the window waits, like a merge. What the new files do not keep is listed under [Limits](#limits).

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
- **A finished split is written, not yet safe on the disk.** A merged or extracted file is made safe on the disk before the operation ends, and a failure to get it there is reported. The files of a split are handed to the system and named, and the split reports success without waiting for the disk. So:
  - If the power goes during a split or just after it, some of its files may be empty or incomplete.
  - If the system cannot write a file out afterwards (a network share that went away, a quota, a full disk), no error is shown.
  - The original is never touched, so the fix is to split again. **Keep the original until you have opened the new files**: do not delete it just because the split said it worked.

  The reason is time. For 300 one-page files in the build container: about 5 seconds when waiting for the disk after each file, 1 to 2.5 seconds when waiting once at the end, and about 0.02 seconds without waiting.
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
| `crates/danonpdf-desktop/src/merge_list.rs` | The Merge view's state and rules, tested without a window. |
| `crates/danonpdf-desktop/src/split_form.rs` | The Split view's state and rules, tested without a window. |
| `crates/danonpdf-desktop/src/paths.rs` | The rules both views share about names: what to propose, and what may be written where. |
| `crates/danonpdf-desktop/src/app.rs` | Draws the view switch and the Merge view, and turns clicks and dropped files into calls on the state. |
| `crates/danonpdf-desktop/src/split_view.rs` | Draws the Split view. |
| `crates/danonpdf-desktop/src/dialogs.rs`, `drops.rs` | The system's file dialogs, and files dropped on the window. |
| `crates/danonpdf-desktop/src/texts.rs` | Every text the user sees, ready for a translation. |
| `scripts/build-windows.sh` | Builds `dist/danonpdf.exe` and `dist/danonpdf-cli.exe`. |
| `AGENTS.md` | How an AI agent uses `danonpdf-cli` and works on this repository. |

## License

[GPL-3.0](LICENSE).
