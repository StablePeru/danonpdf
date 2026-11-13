# DanonPDF for agents

Use `danonpdf-cli` to work with PDF files without the window. It runs one operation, prints one report and exits. It never asks a question, never reads stdin and never uses the network.

## Quick path

1. Run one command with `--json`, for example `danonpdf-cli merge --json --output OUT.pdf A.pdf B.pdf`.
2. Read the exit code: `0` done, `1` refused or failed, `2` wrong command line.
3. For `0` and `1`, parse the single line on stdout as JSON and test `ok`.

To take pages out of a file, ask how many it has first: `danonpdf-cli info --json IN.pdf`.

To look at a page, draw it as an image: `danonpdf-cli render --json --page 1 --output PAGE.png IN.pdf`. This is the one command that needs the PDFium library: see [The PDFium library](#the-pdfium-library).

## Commands

```text
danonpdf-cli merge   --output <FILE> [--force] [--json] <INPUT> <INPUT>...
danonpdf-cli info    [--json] <INPUT>
danonpdf-cli extract --pages <PAGES> --output <FILE> [--force] [--json] <INPUT>
danonpdf-cli split   [--every <N>] --output-dir <DIR> [--force] [--json] <INPUT>
danonpdf-cli render  --page <N> [--width <PX>] --output <FILE> [--force] [--json] <INPUT>
danonpdf-cli --help | --version | help <COMMAND>
```

| Command | What it does |
| --- | --- |
| `merge` | Joins two or more PDF files, in the order given, into one new file. |
| `info` | Says how many pages a PDF file has. Writes nothing. |
| `extract` | Copies the pages chosen with `--pages`, in the order given, into one new file. |
| `split` | Writes the pages into several files of `--every` pages each. The last one may have fewer. |
| `render` | Draws one page and writes it as a PNG image. See [Images](#images). |

| Part | Meaning |
| --- | --- |
| `<INPUT>` | The PDF file to read. `merge` takes at least two; the others take exactly one. |
| `--output <FILE>` | Where to write the new PDF (`merge`, `extract`) or the PNG image (`render`, and then the name must end in `.png`). Required. |
| `--pages <PAGES>` | Which pages to take (`extract`). Required. See [Pages](#pages). |
| `--every <N>` | How many pages go into each file (`split`). A whole number, 1 or more. Default: 1, one file per page. |
| `--output-dir <DIR>` | The folder for the new files (`split`). Required. Created if it does not exist. |
| `--page <N>` | The page to draw, counted from 1 (`render`). Required. |
| `--width <PX>` | The width of the image in pixels, from 1 to 4096 (`render`). Default: 1024. |
| `--force` | Replace outputs that already exist. |
| `--json` | Report as one JSON object instead of one line of text. |

Put `--` before the inputs if one of them starts with `-`.

## Pages

`--pages` takes a list of items separated by commas. Pages are counted from 1.

| Item | Meaning | Example |
| --- | --- | --- |
| `N` | Page N. | `7` |
| `A-B` | Pages A to B, both included. A must not be above B. | `1-3` |
| `A-` | Page A and every page after it. | `10-` |

`1-3,7,10-` takes pages 1, 2, 3 and 7, and from 10 to the last one. The pages are written in the order given: `3,1` puts page 3 first. Spaces around an item are ignored.

| Mistake | Example | Result |
| --- | --- | --- |
| Empty list or empty item | `1,,3` | Usage error, exit code `2`. |
| Not a number | `abc`, `1-2-3` | Usage error, exit code `2`. |
| Page 0 | `0-3` | Usage error, exit code `2`. |
| Backwards range | `5-3` | Usage error, exit code `2`. |
| The same page twice | `1-5,3` | Usage error, exit code `2`. |
| A page past the end of the document | `9` on 5 pages | Failure `page_out_of_range`, exit code `1`. |

The first five are found in the text alone, before any file is opened. The last one needs the document, so it is reported like any other failure of the operation.

## Split file names

`split` names its files after the input: `<name of the input without its extension>-<NNN>.pdf`, inside `--output-dir`.

- Splitting `report.pdf` into `parts` writes `parts/report-001.pdf`, `parts/report-002.pdf`, and so on, in page order.
- The number starts at 1 and has three digits, or as many as the last file needs from a thousand files on (`report-0001.pdf`).
- Every name is checked before anything is written. If one is taken and `--force` was not given, nothing is written at all.

## Images

`render` draws one page and writes it as a PNG file: 8 bits per channel with alpha, on white paper that is fully opaque.

- `--page` counts from 1. Run `info` first to learn how many pages there are.
- `--width` is the width of the image in pixels, from 1 to 4096. Without it the image is 1024 pixels wide.
- The height is not asked for: it follows from the shape of the page. An A4 page drawn 1024 pixels wide is 1449 pixels high.
- A very tall page is drawn narrower than asked, so that its image is at most 8192 pixels high. **Read `width` and `height` from the report, not from the command line.**
- A page that is turned (`/Rotate`) is drawn turned. A page that states no size is drawn as US Letter.
- `--output` must end in `.png`, in any letter case. The tool adds no extension, and what it writes is a PNG whatever the name says.

| Mistake | Example | Result |
| --- | --- | --- |
| Page 0, or not a whole number | `--page 0`, `--page 1-3` | Usage error, exit code `2`. |
| Width out of range, or not a whole number | `--width 0`, `--width 5000` | Usage error, exit code `2`. |
| Output that does not end in `.png` | `--output page.jpg` | Usage error, exit code `2`. |
| A page past the end of the document | `--page 9` on 5 pages | Failure `page_out_of_range`, exit code `1`. |
| The PDFium library is not there | | Failure `renderer_unavailable`, exit code `1`. |

### The PDFium library

`render` draws with PDFium, a library that it loads from a file when it runs. `merge`, `info`, `extract` and `split` never load it and work without it.

| System | File |
| --- | --- |
| Windows | `pdfium.dll` |
| Linux | `libpdfium.so` |

It looks for that file in one folder only:

1. The folder named by the environment variable `DANONPDF_PDFIUM_DIR`, when it is set and not empty. A relative folder is taken from the current folder.
2. Otherwise, the folder the program itself is in. `dist/` is laid out that way: `danonpdf-cli.exe` next to `pdfium.dll`.

Never the current folder, and never `PATH`. If the file is not there or cannot be loaded, the failure is `renderer_unavailable` and `error.path` is the file that was looked for, as an absolute path, also when the variable names a relative folder. Loading a library runs its code: only point the variable at a folder you trust as much as the program.

One case has no `error.path`: the variable is not set and the program cannot tell which folder it is in, so there is no file to name. The code is still `renderer_unavailable`, and the fix is to set `DANONPDF_PDFIUM_DIR`.

### `info` and `render` can disagree

`render` reads the file with PDFium. Every other command reads it with another library (lopdf). On a sound file they agree. On a damaged file they may not, so a file that `info` counts is not thereby a file that `render` draws, and the other way round. Two cases are pinned by tests:

| File | `info` | `render` |
| --- | --- | --- |
| The end of the file is cut off (the table that says where each object is). | Succeeds, with the page count. | Fails with `read_failed`. |
| The catalog names no page tree. | Fails with `broken_pdf`. | Fails with `page_out_of_range`: for PDFium it is a document with 0 pages. |

Both agree on encryption: a file that needs a password is `encrypted` for every command, and a file that is encrypted with the empty password opens for every command.

## Output

| Exit code | Meaning | With `--json` | Without `--json` |
| --- | --- | --- | --- |
| `0` | Done. | One JSON object on stdout. | One line on stdout. |
| `1` | Refused or failed. | One JSON object on stdout. | `error: ...` on stderr, nothing on stdout. |
| `2` | Wrong command line. | Plain text on stderr, not JSON. | Plain text on stderr. |

If the report cannot be written (stdout or stderr is closed, or the disk is full), the line is dropped and the exit code still tells the result.

With `--json`, stdout holds exactly one object on one line and stderr stays empty, for codes `0` and `1`. `ok` and `operation` always come first.

A success has one shape per command:

```text
{"ok":true,"operation":"merge","output":"OUT.pdf","inputs":2,"pages":12}
{"ok":true,"operation":"info","input":"IN.pdf","pages":12}
{"ok":true,"operation":"extract","output":"OUT.pdf","pages":7}
{"ok":true,"operation":"split","outputs":["DIR/IN-001.pdf","DIR/IN-002.pdf"],"files":2,"pages":12}
{"ok":true,"operation":"render","output":"PAGE.png","page":1,"width":1024,"height":1449}
```

A failure has the same shape for every command:

```text
{"ok":false,"operation":"merge","error":{"code":"output_exists","message":"...","path":"OUT.pdf"}}
{"ok":false,"operation":"split","error":{"code":"save_failed","message":"...","path":"DIR/IN-003.pdf","written":["DIR/IN-001.pdf","DIR/IN-002.pdf"]}}
{"ok":false,"operation":"render","error":{"code":"renderer_unavailable","message":"...","path":"C:\\tools\\danonpdf\\pdfium.dll"}}
```

| Field | Meaning |
| --- | --- |
| `operation` | The command that ran: `merge`, `info`, `extract`, `split` or `render`. |
| `input` | The input path, as given (`info`). |
| `output` | The output path, as given (`merge`, `extract`, `render`). |
| `outputs` | The files written, as paths inside `--output-dir`, in page order (`split`). |
| `inputs` | How many input files were read (`merge`). |
| `files` | How many files were written (`split`). |
| `pages` | How many pages: of the input for `info` and `split`, of the output for `merge` and `extract`. |
| `page` | The page that was drawn, counted from 1 (`render`). |
| `width`, `height` | The size in pixels of the image that was written (`render`). `width` can be below the `--width` asked for: see [Images](#images). |
| `error.code` | One of the stable codes below. Decide on this. |
| `error.message` | The reason, for people. The wording may change. |
| `error.path` | The file the error is about, as given. The key is absent when there is none. For `renderer_unavailable` it is the library file that was looked for, as an absolute path; the key is absent when the program could not tell which folder to look in (see [The PDFium library](#the-pdfium-library)). |
| `error.written` | Only when a `split` stopped after writing some of its files: those files, in order. They are complete and are kept. The key is absent in every other failure. |

A path that is not valid Unicode is written with replacement characters.

## Error codes

| Code | Meaning | What to do |
| --- | --- | --- |
| `output_is_input` | An output is one of the inputs. | Choose another output. `--force` does not help. |
| `output_exists` | An output already exists. `error.path` is the first one found. | Choose another output or folder, or pass `--force` if replacing it is intended. |
| `no_inputs` | There was nothing to merge. | Nothing: this command line never reports it. Fewer than two inputs is refused first as a usage error (exit code `2`, plain text on stderr). The code is listed because it is part of the contract. |
| `read_failed` | An input is missing, unreadable or not a PDF. For `render`, also a file too damaged for PDFium to open. | Check `error.path`. |
| `encrypted` | An input needs a password. For `render`, also an input protected in a way that cannot be opened at all. | Not supported; use an unlocked copy. |
| `broken_pdf` | An input opened, but its structure is damaged. | Check `error.path`. |
| `page_out_of_range` | `--pages` or `--page` names a page past the end of the document. | Run `info` to get the page count, then ask again. There is no `error.path`. |
| `page_repeated` | A page was asked for more than once. | Nothing: this command line never reports it. A `--pages` list with a repeated page is refused first as a usage error (exit code `2`, plain text on stderr). The code is listed because it is part of the contract. |
| `no_pages_left` | The new document would have no pages. | Nothing: this command line never reports it, because it has no command that deletes pages. The code is listed because it is part of the contract. |
| `save_failed` | An output file, or the folder for it, could not be written. | Check that `error.path`, or its folder, can be written. After a `split`, look at `error.written` for files that are already there. |
| `renderer_unavailable` | The PDFium library is missing or cannot be loaded. Only `render` reports it, and nothing is wrong with the input. | Put the library file where `error.path` says, or set `DANONPDF_PDFIUM_DIR` to its folder. Without `error.path`, set the variable. The other commands still work. |
| `render_failed` | The page was not drawn, and no other code says why: PDFium gave up on the file without a reason. Only `render` reports it. | Read `error.message`. Nothing in the command line is wrong (a `--width` that cannot be drawn is a usage error, exit code `2`, and never gets this far); try another copy of the file. |

## Rules it keeps

- It never writes over an input, with or without `--force`. Another spelling of the same file (`./a.pdf`, a symbolic link) still counts as that file; a hard link does not.
- It never replaces an existing file unless `--force` is given.
- It writes each output in one step: a failed operation leaves no half-written file and changes no input.
- `merge`, `extract` and `render` wait until their output is on the disk before they report, and report a failure to get it there as `save_failed`.
- `split` does not wait for the disk: `"ok":true` means every file was written and named, not that it is on the disk yet. A power cut during or just after a `split` can leave files empty or incomplete, and a write error the system only meets later (a network share that went away, a quota, a full disk) is never reported. **Do not delete the input because a `split` returned `ok`.** Read the new files back first: `info` on each one, and the page counts must add up to the input's. That proves what the system holds; only time or a restart proves what the disk holds. If files are wrong or missing, run the `split` again with `--force`.
- `merge`, `info`, `extract` and `render` write everything or nothing. `split` writes its files one after another: if one cannot be saved, it stops, keeps the ones already written and lists them in `error.written`.
- It uses paths exactly as given. It does not add `.pdf` or `.png`, skip inputs by name or drop repeated inputs.
- `render` checks the rules about its output before it looks for the PDFium library: `output_is_input` and `output_exists` are reported the same on a computer without the library.
- `render` never changes its input, draws one page per run, and has no way to take a password.
- Only pages and what they use are carried over. Bookmarks, named destinations, form fields, the tagged structure and the document information of the inputs are not.
- A page that is extracted or split off does not bring other pages with it. A link to a page that is not in the new file stays on its page and leads nowhere.

## Build and test

Everything runs in Docker. Do not run `cargo` on the host.

```sh
docker compose build
docker compose run --rm build cargo fmt --all --check
docker compose run --rm build cargo clippy --workspace --all-targets -- -D warnings
docker compose run --rm build cargo test --workspace
docker compose run --rm build bash scripts/build-windows.sh
```

The last command leaves `dist/danonpdf.exe` (the window) and `dist/danonpdf-cli.exe` (this tool) for Windows, with `dist/pdfium.dll` and its licence texts in `dist/licenses/pdfium/`. Keep them together: `render` looks for the DLL next to the program, and so does the Pages view of the window (the one that shows page thumbnails), and the DLL must not be passed on without its licences. Inside the container, run the tool with:

```sh
docker compose run --rm build cargo run --release -p danonpdf-cli -- merge --json --output OUT.pdf A.pdf B.pdf
```

The container only sees the checkout, so the PDF files must be inside it. `render` works there too: the build image holds `libpdfium.so` and sets `DANONPDF_PDFIUM_DIR` to its folder.

The tests draw real pages with that library. The ones that are about finding and loading it are programs of their own (`crates/danonpdf-core/tests/`, and every run of the tool in `crates/danonpdf-cli/tests/cli.rs`), because a process can load PDFium only once. The window draws its thumbnails on a thread of its own (`crates/danonpdf-desktop/src/thumbnail_worker.rs`); its tests run that thread with a fake engine, and one of them with PDFium.

## Working on the code

| Path | Rule |
| --- | --- |
| `crates/danonpdf-core` | All PDF logic and its tests. No user interface, no output format. |
| `crates/danonpdf-cli` | Parses arguments, calls the core, reports. No PDF logic. |
| `crates/danonpdf-desktop` | Draws the window and calls the core. No PDF logic. |

Every feature lands in the core first, then in the command-line tool, then in the window. The error codes, the operation names and the JSON fields are a contract: add to them, do not rename them. Comments explain why, in English.
