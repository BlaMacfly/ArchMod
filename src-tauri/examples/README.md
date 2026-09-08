# Diagnostic tools

These examples are development tools, not part of the application. They exist to
inspect a running game from a terminal — useful when a profile misbehaves and you
need to know whether the fault is in the recipe, the game build, or ArchMod.

Run them with `cargo run --release --example <name> -- <arguments>`.

## Read-only

Safe on a running game: they never write to its memory.

| Example | What it does |
|---|---|
| `detect` | Prints how ArchMod sees one game: install path, build id, whether it is running, and the PID of the actual game process |
| `detect_all` | Same detection across the whole library — the exact query the interface makes |
| `aob_scan` | Searches byte patterns in a module and reports every match with its offset |
| `table_demo` | Parses a table, runs its scans and resolves its entries, end to end |
| `hook_plan` | Says what detouring an instruction would require: stolen instructions, resume point, suitable caves, blockers — **without writing anything** |
| `chemin` | Pointer path search from an address, each path replayed and verified |
| `launch_plan` | Prints, for every installed game, the exact command ArchMod would hand to Steam to start it — and which Steam client it picked |

## Writes to the game

⚠️ **These modify a running process.** Save your game first. A mistake crashes it.

| Example | What it does |
|---|---|
| `pose_hook` | Installs a real detour, captures a register, then **restores the original bytes**. Every write is read back and checked; the code is restored even when nothing is captured |

`pose_hook` takes `<pid> <module> "<pattern>" [seconds] [match] [offset]`. It
verifies that the trampoline and storage areas are untouched before writing, and
refuses to proceed otherwise.

## Why they are kept

Every non-trivial bug in this project was found with one of these: the Proton
process detection, the dead code path in an outdated table, the read-only page
that made a demo profile fail. They pay for the space they take.
