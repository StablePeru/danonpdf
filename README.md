# DanonPDF

DanonPDF will be a free, fast desktop tool for everyday PDF work: merge, split, rotate, reorder and delete pages with thumbnails, compress, and turn images into PDF. Windows first, Linux later. It is part of DanonAA.

> **Status:** early work. The app can merge PDF files, extract pages from one and split one into several files, from the window or from the command line. The window can show the pages of a PDF as thumbnails, and the command line can draw a page as a PNG image. Pages cannot be rotated, reordered or deleted yet, and the other tools do not exist.

The window has three views, **Merge**, **Split** and **Pages**, chosen with the three buttons at the top. It opens on Merge. Files dropped on the window go to the view that is shown, and each view keeps what it holds while another one is open.

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
- If a split cannot save a file halfway through, it stops, keeps the files already written and says how many there are. To try again, choose another folder or empty that one first: the files that were kept have the names the new split would use.

Both run while the window waits, like a merge. What the new files do not keep is listed under [Limits](#limits).

## Looking at the pages

Click **Pages** at the top of the window.

1. Drag one PDF onto the window, or click **Choose PDF…**.
2. Its pages appear as thumbnails, each with its page number under it, as many per row as the window is wide, in the middle of the window. Scroll to see the rest.

The view only shows pages: it changes no file and writes none.

- **The window keeps answering.** The pages are drawn by a second thread, so the window can be scrolled, resized and switched to another view while they arrive. An empty box with `…` is a page that has not been drawn yet.
- **Only what is on screen is drawn.** The rows in the window, and one more above and below, are asked for; scrolling far away drops what was still waiting and draws what is on screen now. A long document opens as fast as a short one.
- **One file at a time.** A new file replaces the one shown, and is shown from its first page. Of several files dropped together, the first PDF is used, and the line at the bottom says how many were left out.
- **A file that cannot be shown leaves the view empty.** A file that needs a password, is damaged, is not a PDF or has no pages is not shown, and the line at the bottom says why. Unlike in the Split view, the file shown before is closed as soon as another one is chosen.
- **A page that cannot be drawn keeps its place.** Its box says so, with the reason, and the other pages are shown as usual.
- **It needs `pdfium.dll` next to `danonpdf.exe`.** The pages are drawn by the PDFium library (see [PDFium](#pdfium)), which is loaded the first time a file is opened in this view. If the file is missing or does not load, the line at the bottom names the file that was looked for. Merge and Split do not need it and work without it.

What the view does not do is listed under [Limits](#limits).

## Command line

`danonpdf-cli` does the work without a window, so a script or an AI agent can use it. It never asks a question, never waits for input and never uses the network.

```sh
danonpdf-cli merge --output merged.pdf first.pdf second.pdf
danonpdf-cli info report.pdf
danonpdf-cli extract --pages 1-3,7,10- --output part.pdf report.pdf
danonpdf-cli split --every 10 --output-dir parts report.pdf
danonpdf-cli render --page 1 --output page.png report.pdf
danonpdf-cli rotate --degrees 90 --pages 5- --output up.pdf report.pdf
danonpdf-cli delete --pages 2,5-6 --output shorter.pdf report.pdf
```

| Command | What it does | What it prints |
| --- | --- | --- |
| `merge` | Joins the files, in the order given, into one new file. | `Merged 2 files (12 pages) into merged.pdf` |
| `info` | Counts the pages of a file. | `report.pdf has 12 pages` |
| `extract` | Copies the chosen pages, in the order given, into one new file. | `Extracted 7 pages into part.pdf` |
| `split` | Writes the pages into files of `--every` pages each (one page each without it). | `Split 12 pages into 2 files: parts/report-001.pdf to parts/report-002.pdf` |
| `render` | Draws one page and writes it as a PNG image. See [Drawing a page](#drawing-a-page). | `Rendered page 1 (1024 by 1449 pixels) into page.png` |
| `rotate` | Writes every page into one new file, with the chosen pages (all of them without `--pages`) turned 90, 180 or 270 degrees to the right. | `Rotated the chosen pages; up.pdf has 12 pages` |
| `delete` | Writes the pages that were not chosen, in the same order, into one new file. | `Deleted the chosen pages; shorter.pdf has 9 pages` |

To reorder pages, use `extract` and name every page in the new order. This moves page 3 of a file to the front and keeps the rest:

```sh
danonpdf-cli extract --pages 3,1,2,4- --output moved.pdf report.pdf
```

When a command fails, it prints `error: …` on stderr instead. With `--json`, every command prints exactly one JSON object on stdout, for a success and for a failure alike, and nothing on stderr:

```text
{"ok":true,"operation":"merge","output":"merged.pdf","inputs":2,"pages":12}
{"ok":true,"operation":"info","input":"report.pdf","pages":12}
{"ok":true,"operation":"extract","output":"part.pdf","pages":7}
{"ok":true,"operation":"split","outputs":["parts/report-001.pdf","parts/report-002.pdf"],"files":2,"pages":12}
{"ok":true,"operation":"render","output":"page.png","page":1,"width":1024,"height":1449}
{"ok":true,"operation":"rotate","output":"up.pdf","pages":12}
{"ok":true,"operation":"delete","output":"shorter.pdf","pages":9}
{"ok":false,"operation":"merge","error":{"code":"output_exists","message":"…","path":"merged.pdf"}}
```

| Exit code | Meaning |
| --- | --- |
| `0` | The operation succeeded. |
| `1` | The operation was refused or failed. |
| `2` | The command line was wrong. The message is plain text on stderr, also with `--json`. |

### Choosing pages

`--pages` takes a list of items separated by commas. Pages are counted from 1. `extract`, `rotate` and `delete` read it the same way.

| Item | Meaning |
| --- | --- |
| `7` | Page 7. |
| `1-3` | Pages 1 to 3. |
| `10-` | Page 10 and every page after it. |

So `1-3,7,10-` on a 12-page file selects pages 1, 2, 3, 7, 10, 11 and 12. `extract` writes the pages in the order given: `3,1` puts page 3 before page 1. `rotate` and `delete` only look at which pages are named, and keep the order of the file. A list that cannot be read (`5-3`, `1,,3`, page 0, the same page twice) is a wrong command line: exit code `2`. A page past the end of the file is the failure `page_out_of_range`: exit code `1`. `info` tells how many pages there are.

### Drawing a page

`render` draws one page of a file and writes it as a PNG image, so a script or an agent can look at a page without a window.

| Flag | Meaning |
| --- | --- |
| `--page <N>` | The page to draw, counted from 1. Required. |
| `--width <PX>` | The width of the image in pixels, from 1 to 4096. Without it: 1024. The height follows from the shape of the page: an A4 page drawn 1024 pixels wide is 1449 pixels high. |
| `--output <FILE>` | Where to write the image. The name must end in `.png`, in any letter case. Required. |

- **The report gives the real size.** A very tall page is drawn narrower than asked, so that its image is at most 8192 pixels high. Read `width` and `height` from the report, not from the command line.
- **The image is plain.** 8 bits per channel with alpha, on white paper that is fully opaque.
- **Mistakes in the command line end with exit code `2`.** That covers `--page 0`, a `--width` out of range and an `--output` that does not end in `.png`. A page past the end of the file is the failure `page_out_of_range`, with exit code `1`.
- **It needs the PDFium library; the other commands do not.** `render` loads `pdfium.dll` (Windows) or `libpdfium.so` (Linux) from the folder the program is in, or from the folder that the environment variable `DANONPDF_PDFIUM_DIR` names when it is set and not empty. If the library is not there or cannot be loaded, the failure is `renderer_unavailable`, with exit code `1`. See [PDFium](#pdfium).

### What it does and does not do

- It reads every input as given. No file is skipped for its name, and `merge` reads the same file twice when it is given twice.
- It uses output paths exactly as given: it does not add `.pdf` or `.png`. `render` refuses an output that does not end in `.png` instead.
- It never writes over an input, not even with `--force` (a hard link to one is not recognised).
- It replaces an existing output only with `--force`.
- `rotate` and `delete` write a new file and only read the one they are given: neither changes a file in place.
- `rotate` adds its turn to the rotation a page already has: 90 degrees on a page that lies at 270 leaves it upright. Use 270 to turn to the left. Any other `--degrees`, 0 included, is a wrong command line.
- `delete` refuses to delete every page: failure `no_pages_left`, exit code `1`, and nothing is written.
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
- **An image has a largest size.** `render` draws a page at most 4096 pixels wide and 8192 pixels high, one page per run.
- **A damaged file can be judged two ways.** `render` and the Pages view read a file with PDFium; every other operation reads it with another library (lopdf). They agree on sound files, and can disagree on damaged ones. A file whose end is cut off is still counted by `info` and refused by `render`. A file whose catalog names no page tree is damaged for `info`, and a document without pages for `render`. So "it can be merged or split" and "it can be drawn" are two separate questions, and a file that the Split view accepts may be one that the Pages view cannot show, or the other way round.
- **The Pages view only shows.** Pages cannot be selected, rotated, reordered or deleted there yet, and a thumbnail cannot be opened larger.
- **Thumbnails have one size.** A thumbnail is 160 points wide (160 pixels on an ordinary screen, more on a sharper one), in a box 226 points high. A page of another shape is fitted into that box.
- **At most 120 thumbnails are kept.** In a long document, the thumbnails farthest from what is on screen are forgotten, and drawn again when you scroll back to them. That keeps the memory they take at about 17 MiB on an ordinary screen and about 68 MiB on one with twice the pixels. A window so large that it shows more than 120 at once keeps all the ones it shows.
- **A file is read again for every thumbnail.** Each one opens the file, draws a page and closes it. In the build container, 300 thumbnails of a small document took about 0.15 seconds, and about 0.6 seconds for the same pages in a file with 30,000 more objects, three quarters of that spent on opening. A file changed by another program while it is shown gives thumbnails of the new content, or a page that says it cannot be drawn.
- **Only the Pages view has a second thread.** Merging, extracting and splitting still run while the window waits: with very big files the window may stop answering for a few seconds.
- **Closing waits for the page being drawn, for two seconds at most.** A page that is being drawn when the window is closed is finished first, which takes a moment. If it does not finish (the file is on a network share that stopped answering, say), the program closes anyway after two seconds. Nothing can be lost by that: the Pages view only reads files.

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

The last command cross-compiles both Windows programs and leaves them in `dist/`: `danonpdf.exe`, the desktop app, and `danonpdf-cli.exe`, the command-line tool. Next to them it puts `pdfium.dll` and, in `dist/licenses/pdfium/`, the licence texts that go with it (see [PDFium](#pdfium)). Keep the three files and that folder together: the Pages view of `danonpdf.exe` and `danonpdf-cli.exe render` look for `pdfium.dll` in their own folder, and the DLL must not travel without its licences. Run the programs on Windows: a window does not open from inside a container.

Downloaded crates and build output are kept in Docker volumes, not in the checkout. On Linux, build the image with your own user so the files the container writes stay yours:

```sh
HOST_UID=$(id -u) HOST_GID=$(id -g) docker compose build
```

A named volume keeps the owner it had the first time it was used. If you change `HOST_UID` or `HOST_GID` later, remove the volumes once so they are created again with the new owner. This only deletes the downloaded crates and the build output, which the next build fetches and rebuilds:

```sh
docker compose down --volumes
```

## PDFium

To draw a page, DanonPDF uses [PDFium](https://pdfium.googlesource.com/pdfium/), the PDF library of Chromium. It is the only part that is not Rust and not compiled here: it is a prebuilt library that a program loads when it draws a page, `pdfium.dll` on Windows and `libpdfium.so` on Linux. Two things draw pages with it: the Pages view of the window, for its thumbnails, and `danonpdf-cli render`, which writes the image as a PNG file. The Merge and Split views and the other commands of `danonpdf-cli` never load it: they work without the library.

- **Where it comes from.** The [pdfium-binaries](https://github.com/bblanchon/pdfium-binaries) project, which builds PDFium and publishes one release per build.
- **Which one.** The release tagged `chromium/8125`: `pdfium-win-x64.tgz` and `pdfium-linux-x64.tgz`.
- **When it is downloaded.** Only while the build image is made (`docker compose build`), never by the programs. The `Dockerfile` pins the SHA-256 of both archives, and a download that does not match stops the build.
- **Where it ends up.** In the image, under `/opt/pdfium/`. `scripts/build-windows.sh` copies the DLL to `dist/pdfium.dll`.
- **Where a program looks for it.** In its own folder, and nowhere else: not in the current folder and not along `PATH`. The environment variable `DANONPDF_PDFIUM_DIR` names another folder when it is set and not empty; a relative folder is taken from the current folder. The build image sets the variable, so the tests and `cargo run` find `libpdfium.so`.
- **When it is loaded.** By the window, the first time a file is opened in the Pages view, on the thread that draws the thumbnails; by `danonpdf-cli`, when `render` runs. A window that never shows the Pages view never loads it.
- **When it is not there.** `danonpdf-cli render` fails with the error code `renderer_unavailable` and names the file it looked for. The Pages view shows nothing and names that file in the line at the bottom; putting the file in place and choosing the PDF again is enough, without restarting. Nothing is downloaded and nothing else is tried.
- **Trust.** Loading a library runs its code inside the program. Point `DANONPDF_PDFIUM_DIR` only at a folder you trust as much as the program itself.

**Licences.** PDFium is under the BSD 3-Clause and Apache 2.0 licences, and it has other libraries built into it (FreeType, ICU, libjpeg-turbo, OpenJPEG, zlib and more), each under its own permissive licence. Those licences ask for their text to go with every copy of the binary, so `dist/licenses/pdfium/` holds all of them as they come in the release: `licenses/pdfium.txt` for PDFium, one file for each library in it, and `LICENSE`, the MIT licence of the pdfium-binaries packaging. `VERSION` there says which build the DLL is. DanonPDF itself stays under GPL-3.0. The BSD 3-Clause and Apache 2.0 licences are both compatible with version 3 of the GPL (Apache 2.0 is not compatible with version 2).

### Bumping PDFium

1. Pick a release of pdfium-binaries whose build number the `pdfium-render` crate supports: it has one `pdfium_NNNN` feature per build it can bind to.
2. In the `Dockerfile`, set `PDFIUM_BUILD` to that number. In `Cargo.toml`, change the `pdfium_NNNN` feature of `pdfium-render` to the same number.
3. Download the two archives of that release and compute their hashes with `sha256sum`. Compare each with the `digest` that GitHub shows for the asset on the release page. Only if they agree, write them into `PDFIUM_LINUX_X64_SHA256` and `PDFIUM_WIN_X64_SHA256`.
4. Rebuild the image with `docker compose build`, then run the tests and `scripts/build-windows.sh` again.

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
| `crates/danonpdf-core/src/render.rs` | Draws a page as pixels and writes them as a PNG. `render/pdfium.rs` is the only file that knows PDFium. |
| `crates/danonpdf-core/tests/` | Tests of how the PDFium library is found and loaded. Each file is a process of its own, because a process loads PDFium once. |
| `crates/danonpdf-cli` | The command-line tool (`danonpdf-cli`): arguments in, a line or a JSON object out. One file per command (`merge.rs`, `info.rs`, `extract.rs`, `split.rs`, `render.rs`). |
| `crates/danonpdf-cli/tests/cli.rs` | Runs the real program and checks its output and exit codes. |
| `crates/danonpdf-desktop` | The desktop app (`danonpdf`), built with [egui](https://github.com/emilk/egui). |
| `crates/danonpdf-desktop/src/merge_list.rs` | The Merge view's state and rules, tested without a window. |
| `crates/danonpdf-desktop/src/split_form.rs` | The Split view's state and rules, tested without a window. |
| `crates/danonpdf-desktop/src/paths.rs` | The rules both views share about names: what to propose, and what may be written where. |
| `crates/danonpdf-desktop/src/app.rs` | Draws the view switch and the Merge view, and turns clicks and dropped files into calls on the state. |
| `crates/danonpdf-desktop/src/split_view.rs` | Draws the Split view. |
| `crates/danonpdf-desktop/src/thumbnails.rs` | The Pages view's state and rules: which pages are asked for, which are kept. Tested without a window. |
| `crates/danonpdf-desktop/src/thumbnail_worker.rs` | The thread that draws the thumbnails, and the requests and replies it exchanges with the window. Tested with a fake engine and with PDFium. |
| `crates/danonpdf-desktop/src/thumbnails_view.rs` | Draws the Pages view, starts the worker thread and holds the textures. |
| `crates/danonpdf-desktop/src/dialogs.rs`, `drops.rs` | The system's file dialogs, and files dropped on the window. |
| `crates/danonpdf-desktop/src/texts.rs` | Every text the user sees, ready for a translation. |
| `Dockerfile`, `compose.yaml` | The build image: the Rust toolchain, the Windows cross-compiler and the pinned PDFium binaries. |
| `scripts/build-windows.sh` | Builds `dist/danonpdf.exe` and `dist/danonpdf-cli.exe`, and puts `pdfium.dll` and its licences next to them. |
| `AGENTS.md` | How an AI agent uses `danonpdf-cli` and works on this repository. |

## License

[GPL-3.0](LICENSE). The PDFium library that ships next to the programs keeps its own licences: see [PDFium](#pdfium).
