# Vex

A fast code editor written in Rust. It looks like VS Code and edits like Vim.

Vex draws its UI on the GPU with [egui](https://github.com/emilk/egui). It has the VS Code layout (activity bar, file explorer, tabs, breadcrumbs, status bar, Dark+ colors) and a modal editing engine built in from the start.

## Build & run

```bash
cargo run --release              # open the current folder
cargo run --release -- path/     # open a folder
cargo run --release -- file.rs   # open a file (its folder becomes the workspace)
```

## Modes

The status bar badge shows the current mode: **NORMAL**, **INSERT**, **VISUAL** / **V-LINE**, **COMMAND**.

| Normal mode | |
|---|---|
| `h j k l`, arrows | move |
| `w b e` / `W B E` | word forward / back / end |
| `0 ^ $` | line start / first non-blank / end |
| `gg G` `{N}G` | top / bottom / line N |
| `{ }` `%` | paragraph / matching bracket |
| `f t F T` + char, `;` `,` | find on line, repeat |
| `Ctrl-d Ctrl-u Ctrl-f Ctrl-b` | half page / page |
| `i a I A o O` | enter insert mode |
| `x X D C s S r J ~` | quick edits |
| `d c y > <` + motion | operators (`dw`, `c$`, `y}`, `>j` …) |
| `dd cc yy >> <<` | whole lines |
| `iw aw i" a" i( a( i{ a{ i[ i<` | text objects (`ciw`, `di(`, `ya"` …) |
| `p P` | paste after / before |
| `u` / `Ctrl-r` | undo / redo |
| `.` | repeat last change |
| `/ ?` `n N` `* #` | search (regex, smartcase) |
| `v V` | visual / visual line |
| `gt gT` | next / previous tab |
| counts | `3dw`, `5j`, `2dd` … |

| Command line (`:`) | |
|---|---|
| `:w [file]` `:wa` | save |
| `:q` `:q!` `:wq` `:x` | close tab (quits when no tabs are open) |
| `:qa` `:qa!` `:wqa` | quit all |
| `:e <file>` | open file |
| `:N` | go to line N |
| `:s/a/b/g` `:%s/a/b/gi` | replace (regex; `$1`/`\1` groups, `\n` splits lines) |
| `:noh` | clear search highlight |
| `:bn` `:bp` | next / previous tab |

| App shortcuts | |
|---|---|
| `Ctrl+P` | go to file (fuzzy) |
| `Ctrl+Shift+P` | command line |
| `Ctrl+S` | save |
| `Ctrl+B` | toggle sidebar |
| `Ctrl+Tab` / `Ctrl+PageDown` | next tab |
| `Ctrl+=` `Ctrl+-` `Ctrl+0` | zoom editor font |

Yanks are copied to the system clipboard, and `Ctrl+V` pastes from it.

## Layout

```
src/
  main.rs      entry point, CLI args
  app.rs       window layout, input routing, drawing
  vim.rs       modal editing engine (GUI-independent, unit tested)
  buffer.rs    text buffer, file I/O, undo/redo
  syntax.rs    keyword-based syntax highlighting
  explorer.rs  file tree and fuzzy file finder
  theme.rs     Dark+ colors and fonts
  snapshot.rs  dev aid: VEX_SNAPSHOT=out.png VEX_KEYS='…' saves a screenshot
```

Run the tests with `cargo test`.

Patterns use Rust [regex syntax](https://docs.rs/regex/latest/regex/#syntax). A pattern that isn't valid regex is matched as plain text, so `/f(` still works.
