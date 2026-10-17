# Third-party notices

This file ships inside every package. It lists the bundled engine, its network, the
bundled font and asset notices, and how the Rust dependency licenses are produced.

## Distribution status

The application is for the owner's personal use. This phase adds no application
license file and changes none. Giving the application or the engine network to anyone
else is gated on a review of the network terms; that review has not happened
(`network_terms_reviewed` is false in `resources/notices/manifest.json`).

## Engine

- Name: Reckless 0.9.0, tag v0.9.0
- Source commit: 0e92358f5acd66e5ac77b1bf558202e47c515435
- Repository: https://github.com/codedeliveryservice/Reckless.git
- License: AGPL-3.0; confirm against the LICENSE file in the pinned commit before
  any conveyance
- Network: v54-5478683c.nnue, SHA-256
  5478683cb1bababde29ae8f29468a99846726548fc6a0ed54cac40ab6d38efbf
- Network URL: https://github.com/codedeliveryservice/RecklessNetworks/releases/download/networks/v54-5478683c.nnue
- Corresponding source and build materials: the pinned repository, tag and commit,
  built by `tools/engine/build.rs`. `resources/engine/manifest.json` records the
  compiler, enabled Cargo features, flags and binary hash of each shipped build.

## Bundled components

- Inter 4.1, SIL Open Font License 1.1, https://github.com/rsms/inter/releases/tag/v4.1
  (license text: `resources/fonts/OFL.txt`)
- three.js bevel construction, MIT, https://github.com/mrdoob/three.js
  (license text: `resources/licenses/three.txt`)
- Chess piece library and sounds: original work of the project owner

## Rust dependencies

Every locked crate and its license expression come from
`cargo metadata --locked --format-version 1`. `chess-tools notices-check` reads that
output, fails when a crate has no license expression, and fails when this file or
`resources/notices/manifest.json` stops describing the engine, network and bundled
components above.
