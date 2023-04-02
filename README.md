# PGCL

PGCL - *pretty good calculator language* - is a language for basic calculations.
Designed for (*my*) rapid success doing calculations.

It is a functional programming language.

The interpreter uses
[logos](https://github.com/maciejhirsz/logos) for lexing and
[rowan](https://github.com/rust-analyzer/rowan) for parsing.

## TODO

* [x] List
* [x] JSON support
* [x] Pipes
* [ ] Operators as functions
* [ ] Optimizations for groups with floating point numbers
* [x] Import (modules)
* [ ] Standard library
* [ ] System call
* [x] Customize builtin function in imported modules (e.g. restrict access)

## Structures

- Lamdas: `\x = body` or just `\x body` (the `=` is optional)
- Variables: `let x = x in body`, `let (@succ x) = x in body`
- If/else: `if x == 0 then 1 else 2`, `if let (@succ y) = x then y else @zero`
- Atoms: `@something`
- Operators (with operator precedence): e.g. `1 + 2`, `4 * 3`, `x == y`
- Operators as functions: `(+)`
- Something like JSON: `{ x: 0 }`, `{ "x": 0 }`
- Pattern matching: `@something _ == @something 0`
- Pipes: Take the left side or previous result as *first* function argument: `1 | add 2`
- Functions with pattern matching:
  ```
  add @zero y = y
  add (@succ x) y = add x (@succ y)
  ```
- Values:
  - Strings: `@something"`
  - Ints: `1`, `0xFF`, `-1`
  - Floats: `1.0`, `-1.0`
  - Lists: `[10, 10, 10]`
