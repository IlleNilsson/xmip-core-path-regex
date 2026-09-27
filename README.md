# xmip-core-path-regex

Regular-expression path technology: the first match or a named group of a
pattern reads from a text Stream and a match writes into it, linear time, no
document built, for promote, demote, route and process. A technology of
[xmip-core-path](https://github.com/IlleNilsson/xmip-core-path).

`RegexLanguage` is the `PathLanguage` for the language `regex`: a pattern compiles once, and a read scans the Stream's own text, decoded once and never copied; `Pattern` is also the one extraction the `regex` route technology makes.

## Toolchain

`rust-toolchain.toml` pins the toolchain for the whole estate. Do not change it
here.

## Verification

The included workflow is manual-only and calls the versioned shared workflow at
`IlleNilsson/.github@v1`.
