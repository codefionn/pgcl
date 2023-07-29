# PGCL

```
This interpreter and programming language are subject to huge change.
```

PGCL - *pretty good calculator language* - is a language for basic
calculations. Designed for (*my*) rapid success doing calculations. It is also
an educational tool, which tries to visualize how functional programming
languages *could* work.

It is a functional programming language.

Fundamentally this programming language reduces expressions. Expressions are
reduced as long as they cannot be reduced any longer. This means that the ``5 +
2`` reduced is ``7``, but also that ``3 + x`` is still ``3 + x``.

The interpreter is designed in a way, that in debug mode (command line option:
``-v``) each reduce step can be seen granulary. This should help understand the
execution steps that are being done.

And `0.1 + 0.2 == 0.3` is true here ...

The interpreter uses
[logos](https://github.com/maciejhirsz/logos) for lexing and
[rowan](https://github.com/rust-analyzer/rowan) for parsing.

## TODO

* [x] List
* [x] JSON support
* [x] Pipes
* [x] Operators as functions
* [ ] Optimizations for groups with floating point numbers
* [x] Import (modules)
* [ ] Standard library
* [x] System call
* [x] Customize builtin function in imported modules (e.g. restrict access)
* [ ] Write documentation
* [x] Message passing
* [ ] File system access
* [ ] Networking (Server/Client)
* [ ] Immediate execution
* [ ] Make lists and maps more lazy
* [ ] Garbage collect signals

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
  - Strings: `"Test"`
  - Atoms: `@test`
  - Ints: `1`, `0xFF`, `-1`
  - Floats: `1.0`, `-1.0`
  - Lists: `[10, 10, 10]`
