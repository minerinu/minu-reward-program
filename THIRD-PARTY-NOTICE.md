# Third-party licenses and notices

The proposed MINU license applies only to original MINU material, not its dependencies.

`DEPENDENCY-LICENSES.json` inventories 271 locked Rust crates and 240 locked JavaScript packages. The Rust archive checksums were compared with Cargo.lock. This is a conservative lockfile inventory, including build, development and optional dependencies; it does not claim every entry is present in the compiled program.

`third-party-licenses/` contains license texts, supplied notices and source copyright notices from checked dependency packages. Where a Rust crate supplies no license file, its declared license, authors and full standard text or upstream license evidence are retained. JavaScript dependencies are installed separately and are not bundled in this release package. Optional packages not installed locally are identified in the inventory.

Most dependencies declare permissive licenses. The JavaScript dependency `rpc-websockets` 9.3.9 declares LGPL-3.0-only. Its copyright notice and both the LGPL v3 and incorporated GPL v3 texts are included. It remains separately installed through npm. Preserve users' library modification and replacement rights; do not relabel it under the MINU license. Bundling JavaScript dependencies into a future distributed website or application requires a separate review of applicable redistribution and relinking obligations.

`eyes` declares MIT through its package metadata and supplied LICENSE. `text-encoding-utf-8` supplies an Unlicense dedication with Encoding Standard attribution, retained here.

Complete component-specific notices accompany the included compiled Rust program. Retain these notices and all relevant dependency licenses when redistributing it. This inventory and notice collection are a technical review, not a certification of copyright ownership or legal compliance.
