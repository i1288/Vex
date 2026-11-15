<p align="center"><img src="assets/logo.svg" width="128" alt="Vex logo"></p>

# Vex

A fast code editor written in Rust. It looks like VS Code and edits like Vim.

Vex draws its UI on the GPU with [egui](https://github.com/emilk/egui). It has the VS Code layout (activity bar, file explorer, tabs, breadcrumbs, status bar, Dark+ colors) and a modal editing engine built in from the start.

## Download

Prebuilt packages for each release are on the [Releases page](https://github.com/i1288/Vex/releases/latest):

| Platform | File | How to run |
|---|---|---|
| **Windows** (x64) | `vex-…-windows-x86_64.zip` | Unzip and run `vex.exe`. Windows SmartScreen may warn because the app isn't code-signed: click *More info → Run anyway*. |
| **macOS** (Apple Silicon and Intel) | `vex-…-macos-universal.zip` | Unzip and drag `Vex.app` into Applications. It isn't notarized, so the first time, right-click it and choose *Open*. |
| **Linux** (x86_64) | `vex-…-linux-x86_64.tar.gz` | Extract and run `./vex`, or run `./install.sh` to install it to `~/.local/bin` with an app-menu entry. |

To publish a new release, push a version tag (`git tag v0.2.0 && git push origin v0.2.0`). The [release workflow](.github/workflows/release.yml) then builds all three packages on GitHub's machines and attaches them to the release.

## Build from source

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
| `Ctrl-w h/l` `Ctrl-w w` | focus pane left / right / next |
| `Ctrl-w v` `Ctrl-w q` `Ctrl-w o` | split / close / keep only this pane |
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
| `:set …` `:settings` | change options / open settings file |
| `:nnoremap` `:inoremap` … `:source` | mappings and scripts (see below) |
| `:terminal` `:files` `:sidebar` `:keybindings` | app commands |
| `:vsp [file]` `:only` | split pane (side by side) / close other panes |

| App shortcuts | |
|---|---|
| `Ctrl+P` | go to file (fuzzy) |
| `Ctrl+Shift+P` | command line |
| `Ctrl+S` | save |
| `Ctrl+B` | toggle sidebar |
| `Ctrl+\` | split editor |
| `Ctrl+1` … `Ctrl+5` | focus pane N |
| `Ctrl+Tab` / `Ctrl+PageDown` | next tab |
| `Ctrl+=` `Ctrl+-` `Ctrl+0` | zoom editor font |
| `Ctrl+,` | open settings |
| `Ctrl+K` | keyboard shortcuts panel |
| `Alt+Z` | toggle word wrap |
| ``Ctrl+` `` | show / focus / hide the terminal |

Yanks are copied to the system clipboard, and `Ctrl+V` pastes from it. In the editor, `Ctrl+C` copies the selection in visual mode and acts as `Esc` otherwise.

## Custom shortcuts and mappings

**Keyboard Shortcuts panel.** The gear icon at the bottom of the activity bar (or `Ctrl+K`, or `:keybindings`) opens a panel with two tabs:

- **Shortcuts:** every app command and its keys. Click **Change**, press the keys you want, and it's saved. If those keys were used by another command, that command loses them and the panel tells you. **Reset** brings back the default.
- **Vim Mappings:** make keys do something else without writing a script. Pick a mode, type what you press (like `<Space>w`) and what it should do (like `:w<CR>`). There are one-click examples to start from.

Both are saved to `keybindings.toml` in the Vex config folder.

**init.vex scripting.** For Vim users, `init.vex` in the same folder runs at startup like a `.vimrc`. Each line is a `:` command, and saving the file in Vex reloads it. The panel's **Open init.vex** button creates it with examples.

```vim
" ~/.config/vex/init.vex
let mapleader = "<Space>"
inoremap jk <Esc>
nnoremap <Leader>w :w<CR>
nnoremap <Leader>t :terminal<CR>
nnoremap <Leader>f :files<CR>
vnoremap < <gv
set relativenumber
```

Supported: `map` / `noremap` and their `n`, `i`, `v`/`x` variants, `unmap`, `mapclear`, `let mapleader`, `source <file>`, `set`, and any other `:` command. Key notation: `<CR>` `<Esc>` `<Tab>` `<BS>` `<Space>` `<Leader>` `<C-x>` `<lt>` and the arrows. The same commands work at the `:` prompt for the current session. Running `:nmap` lists the active mappings.

Commands useful in mappings: `:terminal`, `:files` (Go to File), `:sidebar`, `:keybindings`, `:vsplit`, `:w`.

## Terminal

``Ctrl+` `` opens your `$SHELL` in a panel below the editor, started in the workspace folder. Press it again to hide the panel, or click the editor to switch focus. It's an xterm-compatible terminal with 256-color and true-color support, so full-screen programs like `less`, `htop` and `vim` work. While it has focus, `Ctrl+C` interrupts the running program, `Ctrl+V` pastes, and the mouse wheel scrolls back through history.

## Settings

`:settings` (or `Ctrl+,`) opens `~/.config/vex/settings.toml`. Saving it applies the changes right away:

```toml
theme = "dark"              # or "light"
font_size = 14.0
tab_size = 4
insert_spaces = true
word_wrap = false
line_numbers = true
relative_line_numbers = false
indent_guides = true
scroll_off = 4
```

`:set` changes options for the current session only, like Vim:
`:set wrap`, `:set ts=2`, `:set noet`, `:set rnu`, `:set nu!` (toggle), `:set ts?` (show), `:set` (show all), `:colorscheme light`.

## Layout

```
src/
  main.rs      entry point, CLI args
  app.rs       window layout, input routing, overlays
  view.rs      editor view: text drawing, soft wrap, mouse
  workspace.rs open buffers, split panes, per-tab cursor and scroll
  terminal.rs  pty-backed shell and xterm key encoding
  vim.rs       modal editing engine (GUI-independent, unit tested)
  buffer.rs    text buffer, file I/O, undo/redo
  syntax.rs    keyword-based syntax highlighting
  explorer.rs  file tree and fuzzy file finder
  settings.rs  settings file and :set options
  keymap.rs    vim key notation and mapping table
  commands.rs  app commands and configurable shortcuts
  keys_panel.rs  the Keyboard Shortcuts panel
  theme.rs     Dark+ / Light+ palettes and fonts
  snapshot.rs  dev aid: VEX_SNAPSHOT=out.png VEX_KEYS='…' saves a screenshot
               (on Wayland, run it with WAYLAND_DISPLAY unset)
```

Run the tests with `cargo test`.

Other folders:

```
assets/      logo (SVG) and app icons, generated by scripts/make_icons.py
packaging/   Linux .desktop entry and installer, macOS Info.plist
scripts/     icon generator and release packaging scripts
```

Patterns use Rust [regex syntax](https://docs.rs/regex/latest/regex/#syntax). A pattern that isn't valid regex is matched as plain text, so `/f(` still works.
