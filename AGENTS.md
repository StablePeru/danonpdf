# DanonPDF for agents

Use `danonpdf-cli` to work with PDF files without the window. It runs one operation, prints one report and exits. It never asks a question, never reads stdin and never uses the network.

## Quick path

1. Run `danonpdf-cli merge --json --output OUT.pdf A.pdf B.pdf`.
2. Read the exit code: `0` merged, `1` refused or failed, `2` wrong command line.
3. For `0` and `1`, parse the single line on stdout as JSON and test `ok`.

## Commands

```text
danonpdf-cli merge --output <FILE> [--force] [--json] <INPUT> <INPUT>...
danonpdf-cli --help | --version | help merge
```

| Part | Meaning |
| --- | --- |
| `<INPUT>` | The PDF files to merge, in this order. At least two. |
| `--output <FILE>` | Where to write the merged PDF. Required. |
| `--force` | Replace the output if it already exists. |
| `--json` | Report as one JSON object instead of one line of text. |

Put `--` before the inputs if one of them starts with `-`.

## Output

| Exit code | Meaning | With `--json` | Without `--json` |
| --- | --- | --- | --- |
| `0` | Merged. | One JSON object on stdout. | One line on stdout. |
| `1` | Refused or failed. | One JSON object on stdout. | `error: ...` on stderr, nothing on stdout. |
| `2` | Wrong command line. | Plain text on stderr, not JSON. | Plain text on stderr. |

With `--json`, stdout holds exactly one object on one line and stderr stays empty, for codes `0` and `1`:

```text
{"ok":true,"operation":"merge","output":"OUT.pdf","inputs":2,"pages":12}
{"ok":false,"operation":"merge","error":{"code":"output_exists","message":"...","path":"OUT.pdf"}}
```

| Field | Meaning |
| --- | --- |
| `output` | The output path, as given. |
| `inputs` | How many input files were read. |
| `pages` | How many pages the merged file has. |
| `error.code` | One of the stable codes below. Decide on this. |
| `error.message` | The reason, for people. The wording may change. |
| `error.path` | The file the error is about. The key is absent when there is none. |

A path that is not valid Unicode is written with replacement characters.

## Error codes

| Code | Meaning | What to do |
| --- | --- | --- |
| `output_is_input` | The output is one of the inputs. | Choose another output. `--force` does not help. |
| `output_exists` | The output already exists. | Choose another output, or pass `--force` if replacing it is intended. |
| `no_inputs` | There was nothing to merge. | Pass at least two inputs. |
| `read_failed` | An input is missing, unreadable or not a PDF. | Check `error.path`. |
| `encrypted` | An input needs a password. | Not supported; use an unlocked copy. |
| `broken_pdf` | An input opened, but its structure is damaged. | Check `error.path`. |
| `save_failed` | The merged file could not be written. | Check that the folder of `error.path` exists and is writable. |

## Rules it keeps

- It never writes over an input, with or without `--force`. Another spelling of the same file (`./a.pdf`, a symbolic link) still counts as that file; a hard link does not.
- It never replaces an existing file unless `--force` is given.
- It writes the output in one step: a failed merge leaves no half-written file and changes no input.
- It uses paths exactly as given. It does not add `.pdf`, skip inputs by name or drop repeated inputs.
- Bookmarks, named destinations, form fields and the tagged structure of the inputs are not carried over.

## Build and test

Everything runs in Docker. Do not run `cargo` on the host.

```sh
docker compose build
docker compose run --rm build cargo fmt --all --check
docker compose run --rm build cargo clippy --workspace --all-targets -- -D warnings
docker compose run --rm build cargo test --workspace
docker compose run --rm build bash scripts/build-windows.sh
```

The last command leaves `dist/danonpdf.exe` (the window) and `dist/danonpdf-cli.exe` (this tool) for Windows. Inside the container, run the tool with:

```sh
docker compose run --rm build cargo run --release -p danonpdf-cli -- merge --json --output OUT.pdf A.pdf B.pdf
```

The container only sees the checkout, so the PDF files must be inside it.

## Working on the code

| Path | Rule |
| --- | --- |
| `crates/danonpdf-core` | All PDF logic and its tests. No user interface, no output format. |
| `crates/danonpdf-cli` | Parses arguments, calls the core, reports. No PDF logic. |
| `crates/danonpdf-desktop` | Draws the window and calls the core. No PDF logic. |

The error codes and the JSON fields are a contract: add to them, do not rename them. Comments explain why, in English.
