# Robsidian Project Context

## 1. Philosophy & Rules
* **Identity**: Rust-based Markdown Note App (Tauri v2).
* **Core Concept**: Hybrid Interface (GUI for viewing, TUI/Helix for editing).
* **Tech Stack**: Rust, Tauri v2, TypeScript, TailwindCSS, Helix, Nushell.
* **Coding Standards**:
    * No `unwrap()`. Use `anyhow::Result`.
    * Async Tauri Commands.
    * Strict typing with `serde`.
    * Design: Keyboard-centric, UNIX-like, "Cool/Hacker" or "Cozy" aesthetics.

## 2. Dependencies (Backend: Rust)
* `tauri`: v2
* `serde`, `serde_json`: Serialization
* `anyhow`, `thiserror`: Error Handling
* `walkdir`: Recursive directory walking

## 3. Directory Structure Strategy
* `src-tauri/src/`: Rust backend code.
* `src/`: Frontend code (HTML/JS/CSS).
* `src-tauri/tauri.conf.json`: Main configuration.

## 4. Architecture Constraints
* File operations (List, Rename, Delete) MUST be done in Rust (Tauri Commands), not JS.
* Opening a file MUST spawn a terminal (xterm/zellij) with Helix, not a web-based editor.

## 5. Current directory Structure
```
C:.
├─.vscode
├─dist
│  └─assets
├─node_modules
│  ├─.bin
│  ├─.vite
│  │  └─deps
│  ├─.vite-temp
│  ├─@esbuild
│  │  └─win32-x64
│  ├─@rollup
│  │  ├─rollup-win32-x64-gnu
│  │  └─rollup-win32-x64-msvc
│  ├─@tauri-apps
│  │  ├─api
│  │  │  ├─external
│  │  │  │  └─tslib
│  │  │  └─menu
│  │  ├─cli
│  │  ├─cli-win32-x64-msvc
│  │  └─plugin-opener
│  │      └─dist-js
│  ├─@types
│  │  └─estree
│  ├─esbuild
│  │  ├─bin
│  │  └─lib
│  ├─fdir
│  │  └─dist
│  ├─nanoid
│  │  ├─async
│  │  ├─bin
│  │  ├─non-secure
│  │  └─url-alphabet
│  ├─picocolors
│  ├─picomatch
│  │  └─lib
│  ├─postcss
│  │  └─lib
│  ├─rollup
│  │  └─dist
│  │      ├─bin
│  │      ├─es
│  │      │  └─shared
│  │      └─shared
│  ├─source-map-js
│  │  └─lib
│  ├─tinyglobby
│  │  └─dist
│  ├─typescript
│  │  ├─bin
│  │  └─lib
│  │      ├─cs
│  │      ├─de
│  │      ├─es
│  │      ├─fr
│  │      ├─it
│  │      ├─ja
│  │      ├─ko
│  │      ├─pl
│  │      ├─pt-br
│  │      ├─ru
│  │      ├─tr
│  │      ├─zh-cn
│  │      └─zh-tw
│  └─vite
│      ├─bin
│      ├─dist
│      │  ├─client
│      │  ├─node
│      │  │  └─chunks
│      │  └─node-cjs
│      ├─misc
│      └─types
│          └─internal
├─src
│  └─assets
└─src-tauri
    ├─capabilities
    ├─gen
    │  └─schemas
    ├─icons
    │  ├─android
    │  │  ├─mipmap-anydpi-v26
    │  │  ├─mipmap-hdpi
    │  │  ├─mipmap-mdpi
    │  │  ├─mipmap-xhdpi
    │  │  ├─mipmap-xxhdpi
    │  │  ├─mipmap-xxxhdpi
    │  │  └─values
    │  └─ios
    ├─src
    └─target
        ├─debug
        │  ├─.fingerprint
        │  │  ├─adler2-a9868c068d20acbb
        │  │  ├─aho-corasick-6a0ea717a56b606e
        │  │  ├─alloc-no-stdlib-38ee54d299b786ce
        │  │  ├─alloc-stdlib-c8798d9da92c5f66
        │  │  ├─anyhow-2d7b9155897dc4c5
        │  │  ├─anyhow-46efa223a44c1d55
        │  │  ├─anyhow-9d897c6c1118ca49
        │  │  ├─autocfg-dc3190a37a68ce03
        │  │  ├─base64-c06ce712e720691e
        │  │  ├─bitflags-726920b12e237479
        │  │  ├─bitflags-a3c0f06466a9f025
        │  │  ├─block-buffer-68e3c1a5c86caa3b
        │  │  ├─brotli-c1f0c58d97c3a9fc
        │  │  ├─brotli-decompressor-20f37760ae502763
        │  │  ├─byteorder-523a549ff06550cb
        │  │  ├─bytes-4cebc9075c88fd6d
        │  │  ├─camino-44366fd91b392184
        │  │  ├─camino-923835681d9cbfd9
        │  │  ├─camino-d3765da741fdda64
        │  │  ├─cargo-platform-05134ea34858a8da
        │  │  ├─cargo_metadata-50abd8f87cba7b3d
        │  │  ├─cargo_toml-08d004a6d6c7cac9
        │  │  ├─cfb-ddf00ad07e425929
        │  │  ├─cfb-f5283ab2714a8abd
        │  │  ├─cfg-if-86f350c8a96e16dd
        │  │  ├─convert_case-610b5cfa4a44fd99
        │  │  ├─cookie-28d8535a94f403ad
        │  │  ├─cookie-40c3ddeccbeddf62
        │  │  ├─cookie-507065fbfa0e09d7
        │  │  ├─cpufeatures-8ea99093a383ae5e
        │  │  ├─crc32fast-81decdd33a4ae4f5
        │  │  ├─crc32fast-b34eb6e2fa248e0c
        │  │  ├─crc32fast-bde1739c47564cd7
        │  │  ├─crossbeam-channel-3d5171b79e1ba16a
        │  │  ├─crossbeam-utils-28bbefa286ef49ca
        │  │  ├─crossbeam-utils-d8a2e4cea7cf671d
        │  │  ├─crossbeam-utils-f038aec8fbddbaac
        │  │  ├─crypto-common-32452a2fb6ff712b
        │  │  ├─cssparser-a206ee779b57cb68
        │  │  ├─cssparser-a55c634680114cdd
        │  │  ├─cssparser-c57802a829a26a66
        │  │  ├─cssparser-macros-32d6b6ce4f92d86a
        │  │  ├─ctor-6f789691baff3c94
        │  │  ├─darling-4880ba93bd514093
        │  │  ├─darling_core-ed7d45fb596f4dac
        │  │  ├─darling_macro-dc07b0a3a22487e7
        │  │  ├─deranged-c949ecfd5364fb6e
        │  │  ├─derive_more-0b1979a8810a7913
        │  │  ├─digest-324dfd939a7fb978
        │  │  ├─dirs-3e3f2bf140c194e6
        │  │  ├─dirs-71698fb66cde6e43
        │  │  ├─dirs-sys-78d5b8ed5f8545f4
        │  │  ├─dirs-sys-8acb9e4fc386f55d
        │  │  ├─displaydoc-8be89e4cfebb6383
        │  │  ├─dpi-519a6e1452a80e57
        │  │  ├─dtoa-8efee76198e69e08
        │  │  ├─dtoa-short-3dcab0a8e32cd792
        │  │  ├─dunce-5f825a821c020784
        │  │  ├─dyn-clone-094ee85f751b2c92
        │  │  ├─embed-resource-4829769dcdb07922
        │  │  ├─equivalent-d2050f2ecc448d4b
        │  │  ├─erased-serde-0c024927a8b8cbcc
        │  │  ├─erased-serde-b2a7d94f61a54692
        │  │  ├─erased-serde-d730da3955d95a29
        │  │  ├─erased-serde-ff7ad21916b00bc7
        │  │  ├─fdeflate-2493c1d304de356f
        │  │  ├─flate2-9e4ea8fdd069af93
        │  │  ├─fnv-8fcfdf196f4349b2
        │  │  ├─font-loader-b0274b865d5a6a46
        │  │  ├─form_urlencoded-1b85663513804a20
        │  │  ├─form_urlencoded-a7c3e5abfb7cdea0
        │  │  ├─futf-667ea0799634f8f0
        │  │  ├─fxhash-4b9921be002cbbe0
        │  │  ├─generic-array-696c4c0c72b129c3
        │  │  ├─generic-array-b975a06cbde3b79f
        │  │  ├─generic-array-d259256aaf9fb02c
        │  │  ├─getrandom-1bf49d295b658199
        │  │  ├─getrandom-31ee0c9f3a34991f
        │  │  ├─getrandom-4797e2f86a15005a
        │  │  ├─getrandom-a7c8a761f2c5ad66
        │  │  ├─getrandom-ac13a08a166694e8
        │  │  ├─getrandom-c0cb20036b4832af
        │  │  ├─getrandom-cc9682f0f0c80c08
        │  │  ├─glob-626ed8d481ed5c15
        │  │  ├─hashbrown-095c6671d1f2a39c
        │  │  ├─hashbrown-702ae309fd2ff572
        │  │  ├─heck-65f5062991a6a716
        │  │  ├─html5ever-2b14dda1906aebbe
        │  │  ├─http-1bf0f21b4e82bbf3
        │  │  ├─ico-2bc8d726e7cfcaca
        │  │  ├─icu_collections-ccd528c55bfc23dd
        │  │  ├─icu_collections-d474df864323d615
        │  │  ├─icu_locale_core-29290b9eb4281c9c
        │  │  ├─icu_locale_core-960dc27188d6a780
        │  │  ├─icu_normalizer-72dd11194bc7a819
        │  │  ├─icu_normalizer-a3d6e510f1438391
        │  │  ├─icu_normalizer_data-3115ec9d3af7e2f5
        │  │  ├─icu_normalizer_data-8a6c0c35ba0ef96e
        │  │  ├─icu_normalizer_data-e756e71179792629
        │  │  ├─icu_properties-187bdf7fc4ec2663
        │  │  ├─icu_properties-2027da670983eee9
        │  │  ├─icu_properties_data-43c1713a0e0651cc
        │  │  ├─icu_properties_data-5a0430e053b1b597
        │  │  ├─icu_properties_data-d2e5cbb6d3add255
        │  │  ├─icu_provider-620f5a870cf78d57
        │  │  ├─icu_provider-eee83090046ccbb8
        │  │  ├─ident_case-88755f10aa5410aa
        │  │  ├─idna-26b43f60af496237
        │  │  ├─idna-b9df05a9089f8242
        │  │  ├─idna_adapter-a2162e1b308c9a35
        │  │  ├─idna_adapter-f51904cc36469d59
        │  │  ├─indexmap-043bbe39cc7317cf
        │  │  ├─indexmap-06f706974edf06a1
        │  │  ├─indexmap-4ab9094491b90d57
        │  │  ├─indexmap-ed4e59bed4221ae1
        │  │  ├─infer-13d8ee624ec23b5b
        │  │  ├─infer-46ef9a50768ead03
        │  │  ├─itoa-bade4ade3feaa574
        │  │  ├─json-patch-34b40cf7932b52dc
        │  │  ├─json-patch-8e2c6a8bf0b0f714
        │  │  ├─jsonptr-c7c5edf3d651f496
        │  │  ├─jsonptr-ddcac53e3f1f5e79
        │  │  ├─keyboard-types-5cbf32c898b85ed5
        │  │  ├─kuchikiki-762faed562c78b73
        │  │  ├─lazy_static-1fc70e9ffe0c8836
        │  │  ├─libc-2cddad7221875cd8
        │  │  ├─libc-eca08470acd6bb4d
        │  │  ├─libc-f59c9dcf95e66eab
        │  │  ├─litemap-01a5fa1620234fc2
        │  │  ├─lock_api-e2236e6704b2c63d
        │  │  ├─log-4558f05f6d7f51cd
        │  │  ├─mac-76e5fdaa54633fce
        │  │  ├─markup5ever-668d9fba85620ad1
        │  │  ├─markup5ever-75dca5321f2fa240
        │  │  ├─markup5ever-be77785c51ff33d8
        │  │  ├─matches-9fbf31b8f149aec5
        │  │  ├─match_token-036480c28c34e0fa
        │  │  ├─memchr-71d7cc137f98d297
        │  │  ├─mime-958e79c40af11bdd
        │  │  ├─miniz_oxide-89c235e96115f9f1
        │  │  ├─muda-000bfc9c5655b7fb
        │  │  ├─muda-f6144e2e03d4c115
        │  │  ├─new_debug_unreachable-aff132a323044a2a
        │  │  ├─nodrop-c23e4c51aad02322
        │  │  ├─num-conv-fe85742a46b77b45
        │  │  ├─once_cell-adb2167452e449d9
        │  │  ├─open-3fcea9f8e7a3c992
        │  │  ├─option-ext-6761ca764cf90352
        │  │  ├─parking_lot-2f1ca194afde3f74
        │  │  ├─parking_lot_core-021b55757703afe8
        │  │  ├─parking_lot_core-5a58105c7624973f
        │  │  ├─parking_lot_core-ac1dfa3a1526baa2
        │  │  ├─percent-encoding-1cadef4d5e296769
        │  │  ├─percent-encoding-211eb937c46839ab
        │  │  ├─phf-0fb5c709b4a5cefd
        │  │  ├─phf-3357f2309a74228a
        │  │  ├─phf-f68742f21cfdb52a
        │  │  ├─phf-fd5a0c93e1f27600
        │  │  ├─phf_codegen-4e8a9e2282ea9358
        │  │  ├─phf_codegen-87c824ebe544f94a
        │  │  ├─phf_generator-3759a83757d8c9e2
        │  │  ├─phf_generator-492c4a734abdac98
        │  │  ├─phf_generator-665485bd04b78199
        │  │  ├─phf_macros-02c419b9aca3e2db
        │  │  ├─phf_macros-49b0504edcbbddac
        │  │  ├─phf_shared-122a9eef06c89a02
        │  │  ├─phf_shared-399b3638d31787b4
        │  │  ├─phf_shared-8077f22f001d8dbf
        │  │  ├─phf_shared-d4b70133a5c8bc8b
        │  │  ├─pin-project-lite-a3ff8da2d5cddd23
        │  │  ├─png-4598d45432de1cc5
        │  │  ├─potential_utf-64a55499d6a05186
        │  │  ├─potential_utf-c6e2b048f2bc10c0
        │  │  ├─powerfmt-3a5d748951051c86
        │  │  ├─ppv-lite86-f639dff46d97c788
        │  │  ├─precomputed-hash-996e763f09560676
        │  │  ├─proc-macro-hack-3ae79f8d2cf80ebd
        │  │  ├─proc-macro-hack-8ac3b1dd6f9abce8
        │  │  ├─proc-macro-hack-fb01130da9b38582
        │  │  ├─proc-macro2-32b8c5fdc2f5e46e
        │  │  ├─proc-macro2-5fb39d1838edf370
        │  │  ├─proc-macro2-f508c884c590de41
        │  │  ├─quote-5e0b8cdfecc84ea4
        │  │  ├─quote-b21d46ff5b9707bb
        │  │  ├─quote-d75df690fde05c64
        │  │  ├─rand-3c7070c42c1b8cad
        │  │  ├─rand-c291268283ae62f5
        │  │  ├─rand_chacha-5c4819e22a79d3d5
        │  │  ├─rand_chacha-8f859c3e27521955
        │  │  ├─rand_core-7e1b13e783c82f90
        │  │  ├─rand_core-f18a7bea264c453b
        │  │  ├─rand_pcg-7db6c095c876b2fa
        │  │  ├─raw-window-handle-6ddd1292ced37c29
        │  │  ├─regex-a09b9432b865c949
        │  │  ├─regex-automata-523f7f75cd3c7232
        │  │  ├─regex-syntax-34f8276662a03132
        │  │  ├─rfd-0cc294436cc4a467
        │  │  ├─rfd-9af0e97e2efe6dd0
        │  │  ├─rfd-dc94d740c40df822
        │  │  ├─robsidian-208071733fe41044
        │  │  ├─robsidian-226e1d9d8c524b9c
        │  │  ├─robsidian-36ba92346d2e060d
        │  │  ├─robsidian-4329534e8075a635
        │  │  ├─robsidian-806f63f78905b5f5
        │  │  ├─robsidian-c83cd724980e5339
        │  │  ├─robsidian-f72a02f36199ac24
        │  │  ├─rustc_version-d83c9bb3a9e11504
        │  │  ├─same-file-2952d9bb62344183
        │  │  ├─same-file-8f2f835415b6c321
        │  │  ├─schemars-43d8b762ae5ff7ab
        │  │  ├─schemars-9063a72948757f6b
        │  │  ├─schemars-9c8d873434d4c3ce
        │  │  ├─schemars_derive-06fe2d9939b6d4a9
        │  │  ├─scopeguard-ff9a34b7e67c160f
        │  │  ├─selectors-5d783b6a51f0c6e5
        │  │  ├─selectors-60e67ec753db573c
        │  │  ├─selectors-64e16ec464faa314
        │  │  ├─semver-4bcaff918e7cb249
        │  │  ├─semver-810a113e0fe29616
        │  │  ├─serde-1c0a33a97bca744c
        │  │  ├─serde-2b450620e6513564
        │  │  ├─serde-41f1e265b45d2880
        │  │  ├─serde-67eaf2918c385f3d
        │  │  ├─serde-f0d2cc0d46990c7b
        │  │  ├─serde-f3bfa9797ccd0da7
        │  │  ├─serde-untagged-560748d359887a93
        │  │  ├─serde-untagged-b22680f05ee34be6
        │  │  ├─serde_core-565fa9e1897a2ba7
        │  │  ├─serde_core-b846f07fa670d730
        │  │  ├─serde_core-b851cd66041c7238
        │  │  ├─serde_core-b9f08604cbab6cf3
        │  │  ├─serde_core-d0f5ef39428f5340
        │  │  ├─serde_core-fb37e9241778f4a6
        │  │  ├─serde_derive-fa4ff45a6acde236
        │  │  ├─serde_derive_internals-8874ce5a25add198
        │  │  ├─serde_json-36f215810975671d
        │  │  ├─serde_json-6c20e758eb3c3c48
        │  │  ├─serde_json-9dca4781390209a8
        │  │  ├─serde_json-cb371816fdcde4be
        │  │  ├─serde_json-e13ca2ddb6448de9
        │  │  ├─serde_json-f045fb23aa8bdf6f
        │  │  ├─serde_repr-1f413acd5cf34d3a
        │  │  ├─serde_spanned-545ac73023035828
        │  │  ├─serde_spanned-f4bf0c73b1f02255
        │  │  ├─serde_with-5c7c58134bc429ea
        │  │  ├─serde_with-89f51338fc89c7dd
        │  │  ├─serde_with_macros-fb3f1089966f9614
        │  │  ├─serialize-to-javascript-ae6f43bb78c29e18
        │  │  ├─serialize-to-javascript-impl-93e3f867b6169598
        │  │  ├─servo_arc-728fc31fdc23d35f
        │  │  ├─sha2-3b80bc9dec46c453
        │  │  ├─simd-adler32-adf25f82787fd27e
        │  │  ├─siphasher-583914862d0b02cb
        │  │  ├─siphasher-cf253f5251b231b8
        │  │  ├─smallvec-e62e62464baf5722
        │  │  ├─softbuffer-012ed88c449d7278
        │  │  ├─stable_deref_trait-b1d3c0e92a2d869e
        │  │  ├─stable_deref_trait-ee82389b1d3b22a7
        │  │  ├─string_cache-9405b03100131ad0
        │  │  ├─string_cache_codegen-27a329faa2d504d0
        │  │  ├─strsim-f71fb1fe7ceb2dee
        │  │  ├─syn-5a6117748ff2d667
        │  │  ├─syn-79b0e525365249c2
        │  │  ├─syn-e195c0edb542089e
        │  │  ├─syn-f8f7a59fada8a40d
        │  │  ├─synstructure-aa8a8f9b92fd078b
        │  │  ├─tao-56a7e27894c609fc
        │  │  ├─tauri-2fae059c646ed772
        │  │  ├─tauri-3e1a59c95ab00a90
        │  │  ├─tauri-532a89228101ee2c
        │  │  ├─tauri-9f9ba5878022925f
        │  │  ├─tauri-build-d17dc830b5cf2606
        │  │  ├─tauri-codegen-814c2eb6ebd73561
        │  │  ├─tauri-macros-67803d833303a678
        │  │  ├─tauri-plugin-dialog-27d6f27af11b7386
        │  │  ├─tauri-plugin-dialog-a560c2b21f2cffdd
        │  │  ├─tauri-plugin-dialog-adc5c9e8a6ff65ff
        │  │  ├─tauri-plugin-fa52d9b2b8f48ccb
        │  │  ├─tauri-plugin-fs-3ee9c734964954fd
        │  │  ├─tauri-plugin-fs-9c424cd367d5215d
        │  │  ├─tauri-plugin-fs-c8e1d7ded18df768
        │  │  ├─tauri-plugin-opener-01835f17e331da4e
        │  │  ├─tauri-plugin-opener-2365f64510cf43e8
        │  │  ├─tauri-plugin-opener-7d8005bd6f0f75db
        │  │  ├─tauri-plugin-opener-e116f7bc39d83603
        │  │  ├─tauri-runtime-11a647900767e92d
        │  │  ├─tauri-runtime-a81847dd6b4f4a44
        │  │  ├─tauri-runtime-f39ab84fbc2265d6
        │  │  ├─tauri-runtime-wry-36c09d4bfc1f460f
        │  │  ├─tauri-runtime-wry-8be857ded04e5571
        │  │  ├─tauri-runtime-wry-a230f70bc38d601f
        │  │  ├─tauri-utils-bc28df02c3eb5b46
        │  │  ├─tauri-utils-f87f15e2ad93790a
        │  │  ├─tauri-winres-3be67a281f1d4c07
        │  │  ├─tendril-b2967dcb7c8a4c1d
        │  │  ├─thiserror-5af3972a581cdabc
        │  │  ├─thiserror-a7e17425d026c62d
        │  │  ├─thiserror-b27d0b7f914205fb
        │  │  ├─thiserror-b5ba1f12911d4983
        │  │  ├─thiserror-dbf01df33b7aeced
        │  │  ├─thiserror-e23e7ea06a703c71
        │  │  ├─thiserror-impl-84bc6bc10b31365e
        │  │  ├─thiserror-impl-fb5a68a9d6668edf
        │  │  ├─time-9c58964e412b116f
        │  │  ├─time-core-e63e97133ba53cdd
        │  │  ├─time-macros-c216a3e7e059858a
        │  │  ├─tinystr-6c91706ad33de625
        │  │  ├─tinystr-cf3249768aace149
        │  │  ├─tokio-37487b2162db2ec9
        │  │  ├─toml-20858ffaf932eaef
        │  │  ├─toml-2538c94da05e387a
        │  │  ├─toml_datetime-8e77036a018b34f2
        │  │  ├─toml_datetime-f58c4cc9751f495e
        │  │  ├─toml_parser-28734df35828d2f5
        │  │  ├─toml_writer-20d28397fefdddce
        │  │  ├─tracing-bb483c8fe46e2fe3
        │  │  ├─tracing-core-38e55bb71cbc756f
        │  │  ├─typeid-2a7ed7d7435c2198
        │  │  ├─typeid-6a62b9cef6072e17
        │  │  ├─typeid-fa3665d6e4d2b697
        │  │  ├─typenum-4a59a05ce3f6b6fb
        │  │  ├─typenum-6d2d8738ccac2c85
        │  │  ├─typenum-ad872885fce90da4
        │  │  ├─unic-char-property-30fbbc007685aceb
        │  │  ├─unic-char-range-293f2f8d6c74c8a1
        │  │  ├─unic-common-7d692aea66bebf4c
        │  │  ├─unic-ucd-ident-811e00e434053be2
        │  │  ├─unic-ucd-version-ef763f4522f30fe5
        │  │  ├─unicode-ident-7ba4eaa4c5704e46
        │  │  ├─unicode-segmentation-d26f2ce1b1c4f5b1
        │  │  ├─url-0dcae12aac4d7ab6
        │  │  ├─url-b1a4380a9361c2ad
        │  │  ├─urlpattern-12974685d9f6ebd0
        │  │  ├─urlpattern-723cc26f0c439f27
        │  │  ├─utf-8-3cacced6023e5641
        │  │  ├─utf8_iter-1ba8f22dd04e0f1d
        │  │  ├─uuid-169813e57623d2ca
        │  │  ├─uuid-979a41e9dc60970d
        │  │  ├─version_check-c8e92a0ef81870d5
        │  │  ├─walkdir-6a550a00932635c4
        │  │  ├─walkdir-e1b4eda2a8752e35
        │  │  ├─webview2-com-9a92efa75c6e2afb
        │  │  ├─webview2-com-macros-1bab15e6667e16ab
        │  │  ├─webview2-com-sys-5ff71ff5a0b58197
        │  │  ├─webview2-com-sys-813fa01beea23edd
        │  │  ├─webview2-com-sys-dd9a66ee31c738fc
        │  │  ├─winapi-373f280b2b7d09ec
        │  │  ├─winapi-3b03ae0bd6d0e801
        │  │  ├─winapi-b3162699b8c6e379
        │  │  ├─winapi-util-1c6191ff605f247d
        │  │  ├─winapi-util-a88023517cc2ce80
        │  │  ├─winapi-x86_64-pc-windows-gnu-55c424d815537cf6
        │  │  ├─winapi-x86_64-pc-windows-gnu-b3b2b52e29a133f9
        │  │  ├─winapi-x86_64-pc-windows-gnu-d02e23e1b504805d
        │  │  ├─window-vibrancy-57efcbd00fc682fe
        │  │  ├─windows-ad7d939a2455e0b3
        │  │  ├─windows-collections-acdc4899a1526a38
        │  │  ├─windows-core-e0616aa1727d3b2d
        │  │  ├─windows-future-10a2807c18767a8e
        │  │  ├─windows-implement-b1e7aae9befabef4
        │  │  ├─windows-interface-2b223898b9ac5c44
        │  │  ├─windows-link-32c401c1233c4107
        │  │  ├─windows-link-d7c4f0fbd5465a35
        │  │  ├─windows-numerics-8aebcc446afab69d
        │  │  ├─windows-result-27e0bd04a024cd70
        │  │  ├─windows-strings-0482944818e0f37a
        │  │  ├─windows-sys-1ff36be72e066924
        │  │  ├─windows-sys-304c64db907031cd
        │  │  ├─windows-sys-599e8c72c9a41a0a
        │  │  ├─windows-sys-6b1b7a0a53a2f214
        │  │  ├─windows-sys-de4dfbd291eaec4a
        │  │  ├─windows-targets-85532167a21e6cd8
        │  │  ├─windows-targets-9c7a0b909d43ef36
        │  │  ├─windows-threading-9e673a17c3a78fc4
        │  │  ├─windows-version-922189b913b51150
        │  │  ├─windows_x86_64_gnu-2a67b71b0b50b4c1
        │  │  ├─windows_x86_64_gnu-38b83b4d45ad4781
        │  │  ├─windows_x86_64_gnu-67d43bc69a2e20fa
        │  │  ├─windows_x86_64_gnu-ab7cb84030b2997a
        │  │  ├─windows_x86_64_gnu-b2243bf50bd42ace
        │  │  ├─windows_x86_64_gnu-ea919178c25237a3
        │  │  ├─winnow-16d3b00283a5044b
        │  │  ├─writeable-f4da3c1c33bf0104
        │  │  ├─wry-2b2d85ff85358466
        │  │  ├─wry-7d201a019e37185b
        │  │  ├─wry-a6876f3ba1db2b95
        │  │  ├─yoke-0563cd0d02def843
        │  │  ├─yoke-49564759b1014823
        │  │  ├─yoke-derive-a44b72e0aa2bedcb
        │  │  ├─zerocopy-49746f28598dfa9c
        │  │  ├─zerocopy-57e097a247c6506c
        │  │  ├─zerocopy-a9d44fa9981e2cb6
        │  │  ├─zerofrom-1fd44a7bc6ee371e
        │  │  ├─zerofrom-derive-f9bd724d65773daf
        │  │  ├─zerotrie-8eaf22ab0369a5c4
        │  │  ├─zerotrie-bd8d53f3cc6c41a4
        │  │  ├─zerovec-67d9de0b50b9b8db
        │  │  ├─zerovec-bd50379ecdaa2304
        │  │  ├─zerovec-derive-3daa530a3c0c787a
        │  │  ├─zmij-29eb050bd88b83e6
        │  │  ├─zmij-94db2cfb6358ff6a
        │  │  └─zmij-e93847aac1477b35
        │  ├─build
        │  │  ├─anyhow-2d7b9155897dc4c5
        │  │  ├─anyhow-9d897c6c1118ca49
        │  │  │  └─out
        │  │  ├─camino-44366fd91b392184
        │  │  ├─camino-d3765da741fdda64
        │  │  │  └─out
        │  │  ├─cookie-40c3ddeccbeddf62
        │  │  │  └─out
        │  │  ├─cookie-507065fbfa0e09d7
        │  │  ├─crc32fast-81decdd33a4ae4f5
        │  │  ├─crc32fast-bde1739c47564cd7
        │  │  │  └─out
        │  │  ├─crossbeam-utils-28bbefa286ef49ca
        │  │  ├─crossbeam-utils-f038aec8fbddbaac
        │  │  │  └─out
        │  │  ├─cssparser-a55c634680114cdd
        │  │  │  └─out
        │  │  ├─cssparser-c57802a829a26a66
        │  │  ├─erased-serde-0c024927a8b8cbcc
        │  │  ├─erased-serde-b2a7d94f61a54692
        │  │  │  └─out
        │  │  ├─generic-array-696c4c0c72b129c3
        │  │  ├─generic-array-b975a06cbde3b79f
        │  │  │  └─out
        │  │  ├─getrandom-4797e2f86a15005a
        │  │  ├─getrandom-ac13a08a166694e8
        │  │  │  └─out
        │  │  ├─getrandom-c0cb20036b4832af
        │  │  │  └─out
        │  │  ├─getrandom-cc9682f0f0c80c08
        │  │  ├─icu_normalizer_data-3115ec9d3af7e2f5
        │  │  │  └─out
        │  │  ├─icu_normalizer_data-8a6c0c35ba0ef96e
        │  │  ├─icu_properties_data-5a0430e053b1b597
        │  │  │  └─out
        │  │  ├─icu_properties_data-d2e5cbb6d3add255
        │  │  ├─indexmap-043bbe39cc7317cf
        │  │  │  └─out
        │  │  ├─indexmap-06f706974edf06a1
        │  │  ├─libc-eca08470acd6bb4d
        │  │  ├─libc-f59c9dcf95e66eab
        │  │  │  └─out
        │  │  ├─markup5ever-668d9fba85620ad1
        │  │  ├─markup5ever-75dca5321f2fa240
        │  │  │  └─out
        │  │  ├─parking_lot_core-021b55757703afe8
        │  │  │  └─out
        │  │  ├─parking_lot_core-ac1dfa3a1526baa2
        │  │  ├─proc-macro-hack-8ac3b1dd6f9abce8
        │  │  │  └─out
        │  │  ├─proc-macro-hack-fb01130da9b38582
        │  │  ├─proc-macro2-32b8c5fdc2f5e46e
        │  │  ├─proc-macro2-5fb39d1838edf370
        │  │  │  └─out
        │  │  ├─quote-b21d46ff5b9707bb
        │  │  ├─quote-d75df690fde05c64
        │  │  │  └─out
        │  │  ├─rfd-0cc294436cc4a467
        │  │  ├─rfd-dc94d740c40df822
        │  │  │  └─out
        │  │  ├─robsidian-806f63f78905b5f5
        │  │  ├─robsidian-c83cd724980e5339
        │  │  │  └─out
        │  │  │      └─app-manifest
        │  │  ├─robsidian-f72a02f36199ac24
        │  │  │  └─out
        │  │  │      └─app-manifest
        │  │  ├─schemars-9063a72948757f6b
        │  │  ├─schemars-9c8d873434d4c3ce
        │  │  │  └─out
        │  │  ├─selectors-60e67ec753db573c
        │  │  │  └─out
        │  │  ├─selectors-64e16ec464faa314
        │  │  ├─serde-1c0a33a97bca744c
        │  │  │  └─out
        │  │  ├─serde-2b450620e6513564
        │  │  │  └─out
        │  │  ├─serde-41f1e265b45d2880
        │  │  ├─serde-f3bfa9797ccd0da7
        │  │  ├─serde_core-565fa9e1897a2ba7
        │  │  │  └─out
        │  │  ├─serde_core-b846f07fa670d730
        │  │  ├─serde_core-b851cd66041c7238
        │  │  ├─serde_core-fb37e9241778f4a6
        │  │  │  └─out
        │  │  ├─serde_json-36f215810975671d
        │  │  ├─serde_json-9dca4781390209a8
        │  │  │  └─out
        │  │  ├─serde_json-cb371816fdcde4be
        │  │  │  └─out
        │  │  ├─serde_json-e13ca2ddb6448de9
        │  │  ├─syn-e195c0edb542089e
        │  │  ├─syn-f8f7a59fada8a40d
        │  │  │  └─out
        │  │  ├─tauri-2fae059c646ed772
        │  │  ├─tauri-9f9ba5878022925f
        │  │  │  └─out
        │  │  │      └─permissions
        │  │  │          ├─app
        │  │  │          │  └─autogenerated
        │  │  │          │      └─commands
        │  │  │          ├─event
        │  │  │          │  └─autogenerated
        │  │  │          │      └─commands
        │  │  │          ├─image
        │  │  │          │  └─autogenerated
        │  │  │          │      └─commands
        │  │  │          ├─menu
        │  │  │          │  └─autogenerated
        │  │  │          │      └─commands
        │  │  │          ├─path
        │  │  │          │  └─autogenerated
        │  │  │          │      └─commands
        │  │  │          ├─resources
        │  │  │          │  └─autogenerated
        │  │  │          │      └─commands
        │  │  │          ├─tray
        │  │  │          │  └─autogenerated
        │  │  │          │      └─commands
        │  │  │          ├─webview
        │  │  │          │  └─autogenerated
        │  │  │          │      └─commands
        │  │  │          └─window
        │  │  │              └─autogenerated
        │  │  │                  └─commands
        │  │  ├─tauri-plugin-dialog-a560c2b21f2cffdd
        │  │  ├─tauri-plugin-dialog-adc5c9e8a6ff65ff
        │  │  │  └─out
        │  │  ├─tauri-plugin-fs-3ee9c734964954fd
        │  │  │  └─out
        │  │  ├─tauri-plugin-fs-c8e1d7ded18df768
        │  │  ├─tauri-plugin-opener-7d8005bd6f0f75db
        │  │  │  └─out
        │  │  ├─tauri-plugin-opener-e116f7bc39d83603
        │  │  ├─tauri-runtime-a81847dd6b4f4a44
        │  │  ├─tauri-runtime-f39ab84fbc2265d6
        │  │  │  └─out
        │  │  ├─tauri-runtime-wry-36c09d4bfc1f460f
        │  │  ├─tauri-runtime-wry-a230f70bc38d601f
        │  │  │  └─out
        │  │  ├─thiserror-5af3972a581cdabc
        │  │  │  └─out
        │  │  ├─thiserror-a7e17425d026c62d
        │  │  │  └─out
        │  │  ├─thiserror-b5ba1f12911d4983
        │  │  ├─thiserror-dbf01df33b7aeced
        │  │  ├─typeid-2a7ed7d7435c2198
        │  │  │  └─out
        │  │  ├─typeid-6a62b9cef6072e17
        │  │  ├─typenum-4a59a05ce3f6b6fb
        │  │  ├─typenum-ad872885fce90da4
        │  │  │  └─out
        │  │  ├─webview2-com-sys-5ff71ff5a0b58197
        │  │  │  └─out
        │  │  │      ├─arm64
        │  │  │      ├─x64
        │  │  │      └─x86
        │  │  ├─webview2-com-sys-813fa01beea23edd
        │  │  ├─winapi-373f280b2b7d09ec
        │  │  │  └─out
        │  │  ├─winapi-3b03ae0bd6d0e801
        │  │  ├─winapi-x86_64-pc-windows-gnu-b3b2b52e29a133f9
        │  │  │  └─out
        │  │  ├─winapi-x86_64-pc-windows-gnu-d02e23e1b504805d
        │  │  ├─windows_x86_64_gnu-38b83b4d45ad4781
        │  │  │  └─out
        │  │  ├─windows_x86_64_gnu-67d43bc69a2e20fa
        │  │  ├─windows_x86_64_gnu-ab7cb84030b2997a
        │  │  ├─windows_x86_64_gnu-ea919178c25237a3
        │  │  │  └─out
        │  │  ├─wry-2b2d85ff85358466
        │  │  ├─wry-a6876f3ba1db2b95
        │  │  │  └─out
        │  │  ├─zerocopy-49746f28598dfa9c
        │  │  ├─zerocopy-57e097a247c6506c
        │  │  │  └─out
        │  │  ├─zmij-94db2cfb6358ff6a
        │  │  │  └─out
        │  │  └─zmij-e93847aac1477b35
        │  ├─deps
        │  │  ├─rmetaBQ7hlb
        │  │  ├─rmetaHD0IJJ
        │  │  ├─rmetajhQEZP
        │  │  └─rmetaqZIrwF
        │  ├─examples
        │  └─incremental
        │      ├─build_script_build-083h39y91uqar
        │      │  └─s-hf0n9r3ovb-12t3gky-ct8pen1uohgmb2zipn4osiax7
        │      ├─robsidian-0iv9syvkfv0aa
        │      │  ├─s-hf0qfbsytp-1mdxjp2-85n1iik71ga9apmyod0n1h431
        │      │  └─s-hf0qi3bjgo-1lq3q1l-8j6iwkrkhm60bvsbztso4l04m
        │      ├─robsidian-28t29m1wns1lw
        │      │  ├─s-hf0p7igqxz-1h1qbk9-dncxj5xep3qduhnn157iifkwt
        │      │  └─s-hf0pxe7ogx-07qugx1-1i9c8g5h38770aiu4z6wt5hek
        │      ├─robsidian-3cq844urvyqmq
        │      │  ├─s-hf1hu2nxgt-0kjld53-780idzh54vms5zdnc9x40xv2v
        │      │  └─s-hf1judhz3e-1vnl441-780idzh54vms5zdnc9x40xv2v
        │      ├─robsidian_lib-0lnjag9d6292h
        │      │  ├─s-hf0p78ayu7-0dzuc2b-cki9c0539moshwugtf2imjslq
        │      │  └─s-hf0px4i2fn-06u4lio-emwplu04urq2dc8c25hoosoxf
        │      ├─robsidian_lib-15o8eb9t3z4zr
        │      │  ├─s-hf0qhlee36-1h5bs4i-7ow0box7a0vukz29lszrosl3e
        │      │  └─s-hf0qhtxsyq-0u5oxb8-chlj9lfv5iey59rv9trhjo47g
        │      └─robsidian_lib-3qk8z2te34uk5
        │          ├─s-hf1htszvnz-1k4jy6d-a2iv2oii9qlq2b05belv6ej9a
        │          └─s-hf1ju602v7-0wgdddm-a2iv2oii9qlq2b05belv6ej9a
        └─release
            ├─.fingerprint
            │  ├─adler2-4e844b2dd1ccdb8e
            │  ├─aho-corasick-62920ee0f725eff4
            │  ├─aho-corasick-e5f00ca3e1c6206f
            │  ├─alloc-no-stdlib-3cea52eac37515d0
            │  ├─alloc-no-stdlib-a6226f41606c63e0
            │  ├─alloc-stdlib-0dfef65f338752b2
            │  ├─alloc-stdlib-654b00f26a1f748b
            │  ├─anyhow-1111dd729a3da0c8
            │  ├─anyhow-667808f96d0dc36a
            │  ├─anyhow-c9cef7f0dc3b0bf2
            │  ├─anyhow-f2bbce3567efcd3a
            │  ├─anyhow-f73699a6bba4c3b5
            │  ├─autocfg-ced9788861b59f4c
            │  ├─base64-4a67d351ce60285a
            │  ├─bitflags-35090803e33a837e
            │  ├─bitflags-b8a9d4c3cf02b0ef
            │  ├─block-buffer-93fc835648f5bca6
            │  ├─brotli-5490f9d179c6819c
            │  ├─brotli-57875fa3a3d2ff9b
            │  ├─brotli-decompressor-20cc2b71f804af50
            │  ├─brotli-decompressor-742ad60e34d0277c
            │  ├─byteorder-63d4be1ff892b379
            │  ├─byteorder-75e7643e77550602
            │  ├─bytes-093edb3c179f1b69
            │  ├─bytes-d357e0ff1dfdc9e2
            │  ├─camino-a899df70fd1ff3a4
            │  ├─camino-b29df80ae03e5619
            │  ├─camino-e602a7cbee1d38c4
            │  ├─cargo-platform-3ad2caf594b03a61
            │  ├─cargo_metadata-4e5472437deb497e
            │  ├─cargo_toml-f49931320ba543d4
            │  ├─cfb-340d720034187772
            │  ├─cfb-9df8120a2d381dc5
            │  ├─cfg-if-6f67fe0d2c91c230
            │  ├─cfg-if-a2274fade5eaf49d
            │  ├─convert_case-e0680b01e2a08ddb
            │  ├─cookie-430f7ff2bf20da77
            │  ├─cookie-b44e032133bb9c05
            │  ├─cookie-be7d48e10fe14902
            │  ├─cpufeatures-f1e6a2807f106062
            │  ├─crc32fast-06195fab08cdcfcc
            │  ├─crc32fast-a4e869e04d14a1f2
            │  ├─crc32fast-c02bc35c2179f7e1
            │  ├─crossbeam-channel-8c388a621a248c3a
            │  ├─crossbeam-utils-177169e4a4e73c44
            │  ├─crossbeam-utils-29821470ac683c17
            │  ├─crossbeam-utils-32d8375ed2826317
            │  ├─crypto-common-14f7a3a8d586df12
            │  ├─cssparser-2ae4ceb791e83986
            │  ├─cssparser-462056496bef917e
            │  ├─cssparser-bd5383b13776bba5
            │  ├─cssparser-macros-06763def8e2b9e0c
            │  ├─ctor-922e4af0ad93f787
            │  ├─darling-34b4a78b2e0b8c98
            │  ├─darling_core-fc06f28464599496
            │  ├─darling_macro-168169622935c75f
            │  ├─deranged-96966b9172a4c1db
            │  ├─derive_more-461ecc384de0a79b
            │  ├─digest-620a4abfecafa169
            │  ├─dirs-4b8ed06a68fae0ce
            │  ├─dirs-9ac2c27e9e06f14f
            │  ├─dirs-sys-2bd5fd59b6916efa
            │  ├─dirs-sys-77c00f22c3abf7e9
            │  ├─displaydoc-fc8fd96c61bc3ca8
            │  ├─dpi-11c0b65d206cbebe
            │  ├─dtoa-636cc686b2c69069
            │  ├─dtoa-short-37fcc7cf40139d56
            │  ├─dunce-56df038fb098d61d
            │  ├─dunce-896f3896aafcd951
            │  ├─dyn-clone-d45e12e162f57c55
            │  ├─embed-resource-4bd3d04c70515423
            │  ├─equivalent-2a4d9ade489aac46
            │  ├─erased-serde-4aa5b460abb255d3
            │  ├─erased-serde-4b7d2b5203ffc986
            │  ├─erased-serde-5dc19c6d63c5191b
            │  ├─erased-serde-e2a6922ab5d87498
            │  ├─erased-serde-ec93aa0ec963f067
            │  ├─fdeflate-7835377d23cb013e
            │  ├─flate2-0747da001abacf12
            │  ├─fnv-8452a20c9c06a12e
            │  ├─fnv-c0ec60c63ee7d716
            │  ├─font-loader-2566649158626714
            │  ├─form_urlencoded-8660cb4b39343082
            │  ├─form_urlencoded-e4a678d9ffcbdb56
            │  ├─futf-18bede1742cda6e4
            │  ├─fxhash-05de514b22378b2a
            │  ├─generic-array-0cd79ccd82607478
            │  ├─generic-array-74e3286a7ae683ae
            │  ├─generic-array-bbe403ff67779bff
            │  ├─getrandom-0c8a9c83583f935d
            │  ├─getrandom-1cc08b28c87f29ef
            │  ├─getrandom-327b07940c5ef9e4
            │  ├─getrandom-5ac0d626594f2fe5
            │  ├─getrandom-7ad415ccbb72bb3c
            │  ├─getrandom-9b6e87f8f9d0b178
            │  ├─getrandom-e920f1887873aed1
            │  ├─getrandom-fc60148d22065964
            │  ├─getrandom-fd5676adc65be326
            │  ├─glob-4fb3e84f5b27536f
            │  ├─glob-e633af979d1d9fe9
            │  ├─hashbrown-1812c2adfd19b42b
            │  ├─hashbrown-1d671037849b6a38
            │  ├─heck-156875d22fb78ade
            │  ├─heck-60e4fb05384e8246
            │  ├─html5ever-73f179058bc7d1a2
            │  ├─http-6718682a29b3dd12
            │  ├─http-e77dcf27a716d332
            │  ├─ico-6356be64bda08604
            │  ├─icu_collections-2691b09887ef675d
            │  ├─icu_collections-73bd1aae76b2e579
            │  ├─icu_locale_core-18ddf04e4ab2f343
            │  ├─icu_locale_core-b5519b4d1e088f7f
            │  ├─icu_normalizer-eaa908d726f6c4d9
            │  ├─icu_normalizer-ee6927987c0eacbf
            │  ├─icu_normalizer_data-3ebd831f38976c81
            │  ├─icu_normalizer_data-5a4ab1f6470a17d9
            │  ├─icu_normalizer_data-6b92c08fb3fb48ad
            │  ├─icu_normalizer_data-6d939e015239c461
            │  ├─icu_normalizer_data-cf76dd6e2cc031f7
            │  ├─icu_properties-5a1a6fd790c04213
            │  ├─icu_properties-74aa2008c487c847
            │  ├─icu_properties_data-1c5a222da0fa1028
            │  ├─icu_properties_data-539a6bc240d5f9aa
            │  ├─icu_properties_data-77e9bb239dcb8845
            │  ├─icu_properties_data-7fb271212a193c98
            │  ├─icu_properties_data-801b9bf66c9bf9b2
            │  ├─icu_provider-95b701f82c755e9d
            │  ├─icu_provider-dd26685597b8c41d
            │  ├─ident_case-466aaf2157d8ab47
            │  ├─idna-4f44037b13dbb56f
            │  ├─idna-bdbb1462ef175d62
            │  ├─idna_adapter-21b9500a6e2ebfb4
            │  ├─idna_adapter-d7cb5c39bee32aca
            │  ├─indexmap-b9eccc866a419ea4
            │  ├─indexmap-cf8497a9779a38a6
            │  ├─indexmap-d20cf8c338dd2073
            │  ├─indexmap-e758aca1ad43b735
            │  ├─infer-5837ade9161a4cdc
            │  ├─infer-c2f099ad1faa51b9
            │  ├─itoa-0e49e4c3f095a5c6
            │  ├─itoa-5b74247890531d9a
            │  ├─json-patch-f3acb110927390db
            │  ├─json-patch-fcff12b1110f78a9
            │  ├─jsonptr-1f5132d25ac3c9f9
            │  ├─jsonptr-2c4cc80d500f22fb
            │  ├─keyboard-types-d4fc06e179260530
            │  ├─kuchikiki-fb3b863f2292e165
            │  ├─lazy_static-b15ea54ff9a02f3f
            │  ├─libc-5771c1a422666e18
            │  ├─libc-d533098e101f77ca
            │  ├─libc-d602a6fefe35e36d
            │  ├─litemap-0fd26c1a4d316fe3
            │  ├─litemap-f998c400b94ca698
            │  ├─lock_api-a40ae70355009295
            │  ├─lock_api-b52ca585c7ac34b6
            │  ├─log-2366033b3274fd54
            │  ├─log-27658960bef8ac93
            │  ├─mac-3ed4683fc4e87c3c
            │  ├─markup5ever-0fc6931d9bcd32e2
            │  ├─markup5ever-16d147e9df44d1c6
            │  ├─markup5ever-b1f1a2032d90e809
            │  ├─matches-f269bff5cac2faed
            │  ├─match_token-212296851c0e9230
            │  ├─memchr-a0ec7307099a0b4d
            │  ├─memchr-af2fad784253fcf8
            │  ├─mime-b63831c87f10264f
            │  ├─miniz_oxide-ca3d646f9289dcfe
            │  ├─muda-be57d731d5838938
            │  ├─new_debug_unreachable-029b366895b47baf
            │  ├─nodrop-83774b60739125ce
            │  ├─num-conv-5c6e8386f8433d04
            │  ├─num-conv-72c0092e8687f02b
            │  ├─once_cell-e7ca1e284552c511
            │  ├─open-e3acad8afa2d2331
            │  ├─option-ext-2fd0c3b1d12b1b7e
            │  ├─option-ext-9e8cca4b5b5ad3d6
            │  ├─parking_lot-46e9b5768b273e79
            │  ├─parking_lot-5cd8dc20a7846dd0
            │  ├─parking_lot_core-05578ef725f00621
            │  ├─parking_lot_core-2d4d2aa69d66ea62
            │  ├─parking_lot_core-3975e4ba3d592c63
            │  ├─parking_lot_core-7f12927c95ced88a
            │  ├─parking_lot_core-bd552eedca7689e4
            │  ├─percent-encoding-96042345f081e6d1
            │  ├─percent-encoding-dd794ec8e535a62d
            │  ├─phf-05312e072c86e5be
            │  ├─phf-5c89f61ae81cf41e
            │  ├─phf-a4ff4f1604b8edee
            │  ├─phf-a98a137e6097d7be
            │  ├─phf_codegen-52b19b4aa5636a72
            │  ├─phf_codegen-b883e5ac0d78348b
            │  ├─phf_generator-471eedaa4ea83bfe
            │  ├─phf_generator-6af78ac2639c40f1
            │  ├─phf_generator-bd9f8faea3c7cff9
            │  ├─phf_macros-990f7fc034389ed9
            │  ├─phf_macros-b27818bbdf43b7dc
            │  ├─phf_shared-1084cec8dc9886fc
            │  ├─phf_shared-400b731dfb8d0589
            │  ├─phf_shared-60db89e7e14d1558
            │  ├─phf_shared-cc510b388a68c0b3
            │  ├─pin-project-lite-a27dc8d7deb0bb8c
            │  ├─png-2e4f0565f698544f
            │  ├─potential_utf-23ff07fce86fa613
            │  ├─potential_utf-b2584b43083abc2c
            │  ├─powerfmt-93a635cbb1adc818
            │  ├─ppv-lite86-63025040164b16e0
            │  ├─precomputed-hash-cd6580365de56aae
            │  ├─proc-macro-hack-3f9db64bdd7eeed2
            │  ├─proc-macro-hack-498f4ed39dcd2545
            │  ├─proc-macro-hack-e28c4d3b50728336
            │  ├─proc-macro2-55475e72c7177be5
            │  ├─proc-macro2-641518f2893257a3
            │  ├─proc-macro2-b8ba1343c081c770
            │  ├─quote-0f9325ac0e1f74fe
            │  ├─quote-5ae667968e653285
            │  ├─quote-c6a80555d744b6ba
            │  ├─rand-373ef4be487c56fb
            │  ├─rand-59e9689c7dfab878
            │  ├─rand_chacha-8f93637675632008
            │  ├─rand_chacha-e411d4f857aff686
            │  ├─rand_core-704d02d05b7b7917
            │  ├─rand_core-8b4b1b19448ca988
            │  ├─rand_pcg-996246386f898c65
            │  ├─raw-window-handle-f3d979e92c984447
            │  ├─regex-471c1dea8da60b01
            │  ├─regex-automata-6d9c345d30915d30
            │  ├─regex-automata-842794fbf8e216b4
            │  ├─regex-ed9dd06bd87fda11
            │  ├─regex-syntax-69dec7d07f4e4ea2
            │  ├─regex-syntax-dfd9e9cbf5d2309b
            │  ├─rfd-498561fb827f8530
            │  ├─rfd-8c1a032d115c4b35
            │  ├─rfd-f73c99fdafe2b245
            │  ├─robsidian-208071733fe41044
            │  ├─robsidian-2caee7941fd71216
            │  ├─robsidian-b72987d865d63a37
            │  ├─robsidian-fc896c8f829342d2
            │  ├─rustc_version-caac6496bde2a67f
            │  ├─same-file-0744bb96975861db
            │  ├─same-file-bbb0a2782061309f
            │  ├─schemars-346be731d81b64d3
            │  ├─schemars-6e14ba86cf8603e6
            │  ├─schemars-e46763b2f5c6cb0e
            │  ├─schemars_derive-2ed7f0305d8e7f3c
            │  ├─scopeguard-29d9d1658e91901d
            │  ├─scopeguard-e244dc6ed0c66ced
            │  ├─selectors-23b11b9bb1300037
            │  ├─selectors-55cbd44eeb0fdb50
            │  ├─selectors-a4e629068272f964
            │  ├─semver-0b6317ffc3798c55
            │  ├─semver-42b8f2cdc59d99d0
            │  ├─serde-0c71ddf870e5bbea
            │  ├─serde-3140e029bb1fd9ae
            │  ├─serde-36fbcfa57eb4de88
            │  ├─serde-49e0aad632408faa
            │  ├─serde-cb064accfb94d26f
            │  ├─serde-f4ebe47029e3e674
            │  ├─serde-untagged-4cb147bcb21d3ec9
            │  ├─serde-untagged-77d929a0c31edd4c
            │  ├─serde_core-8daa3bd8c71ed929
            │  ├─serde_core-9b489af8b7a09b15
            │  ├─serde_core-a793789fad403c3e
            │  ├─serde_core-b3aa795f2956ec6d
            │  ├─serde_core-cd6687b5c9e9302e
            │  ├─serde_core-dbf06a3af37b6c67
            │  ├─serde_derive-97c7798cf6b65666
            │  ├─serde_derive_internals-cb09904d510ed01f
            │  ├─serde_json-127e5a0e9d89e4e4
            │  ├─serde_json-2e019e996d1d130f
            │  ├─serde_json-3980b81d2acc2f25
            │  ├─serde_json-5756f94e813f679e
            │  ├─serde_json-d31c4b304a14d93c
            │  ├─serde_json-e69e69fad3e01d9f
            │  ├─serde_repr-431ef0ac634a0043
            │  ├─serde_spanned-098f3c7122a692b9
            │  ├─serde_spanned-1ce1e3ab79cd786d
            │  ├─serde_with-10c31311836d9062
            │  ├─serde_with-dd265d2f67e35830
            │  ├─serde_with_macros-9ae728cda90c654f
            │  ├─serialize-to-javascript-2096372d42fb668c
            │  ├─serialize-to-javascript-impl-3f1f5fc957704613
            │  ├─servo_arc-87e01770ad57c96a
            │  ├─sha2-209cd1dea90e6d74
            │  ├─simd-adler32-99a8f8f7ca9a96c3
            │  ├─siphasher-2a88d7dd4be86f49
            │  ├─siphasher-3c2e81185166a7ce
            │  ├─siphasher-640765205b683e5d
            │  ├─smallvec-5ce401d89a7637c7
            │  ├─smallvec-a1e232e957cb943d
            │  ├─softbuffer-f4b7fe56acd14c7d
            │  ├─stable_deref_trait-30357af53161cf59
            │  ├─stable_deref_trait-4ff5e2ea15cdb507
            │  ├─string_cache-bcd516bc27529051
            │  ├─string_cache_codegen-e0f93e1e72c01454
            │  ├─strsim-80ee49e356ee39a8
            │  ├─syn-b391cc75aec4edbe
            │  ├─syn-b86e907b4e92721c
            │  ├─syn-d9f2e01a1e8e59a1
            │  ├─syn-e2dbf8d5b0cabc91
            │  ├─synstructure-f994c2bf00850c39
            │  ├─tao-b233ec9b333ea5a1
            │  ├─tauri-11d010be585eea5e
            │  ├─tauri-1327a37cf5c10c5c
            │  ├─tauri-build-823f952db017562c
            │  ├─tauri-c40bbf537df5a54a
            │  ├─tauri-codegen-971ced390ea164be
            │  ├─tauri-macros-3e8a9a91b9984029
            │  ├─tauri-plugin-dialog-b60b047a2d1b7843
            │  ├─tauri-plugin-dialog-e5e0fff80ee41af2
            │  ├─tauri-plugin-dialog-ef79495d3dfc59bb
            │  ├─tauri-plugin-f535ffd3538c92ce
            │  ├─tauri-plugin-fs-2a4d46c4cd2a69c8
            │  ├─tauri-plugin-fs-5933ebc79c2776a3
            │  ├─tauri-plugin-fs-8bb525c2a6cb0437
            │  ├─tauri-plugin-opener-35d0a93b7f548171
            │  ├─tauri-plugin-opener-9f13fb926938da48
            │  ├─tauri-plugin-opener-dd082f06fe1adfd3
            │  ├─tauri-runtime-2e7d6839c97384aa
            │  ├─tauri-runtime-59a29b06d9633cc4
            │  ├─tauri-runtime-f32d1faed27cb5b7
            │  ├─tauri-runtime-wry-249f448d012d18c3
            │  ├─tauri-runtime-wry-6ef9fb7eca76d1ca
            │  ├─tauri-runtime-wry-df0e1cd81588220e
            │  ├─tauri-utils-0cd499e4b3891a61
            │  ├─tauri-utils-6f2fd1ba232ff7a3
            │  ├─tauri-winres-4e3f579ccbaa6bc9
            │  ├─tendril-53151ecd5e2824a0
            │  ├─thiserror-137697f7faaac4a1
            │  ├─thiserror-150d6a2088d5d817
            │  ├─thiserror-274cd781366ec4b6
            │  ├─thiserror-4dd67743e5c0fdbd
            │  ├─thiserror-6b3038345d51aefb
            │  ├─thiserror-886ee717f1155ff1
            │  ├─thiserror-b98ce535635ffd97
            │  ├─thiserror-c6f4c2c4eb4837bb
            │  ├─thiserror-eab25de2f8f88e24
            │  ├─thiserror-fbfa1bfaa735fa32
            │  ├─thiserror-impl-3f05c22c36057fb9
            │  ├─thiserror-impl-f71f2d75e878a9fd
            │  ├─time-750a8a779554f212
            │  ├─time-core-39dbc1e764822e97
            │  ├─time-core-eec423281bfa485a
            │  ├─time-macros-bb1a40c5371df325
            │  ├─tinystr-6016c9dee910f2d3
            │  ├─tinystr-e931afd990a40494
            │  ├─tokio-ae59f7fb9458bd72
            │  ├─toml-42c9cfaa4d489b2a
            │  ├─toml-586e49d7387e833b
            │  ├─toml_datetime-d71893b015e99fcb
            │  ├─toml_datetime-fd3229f8312bcd6f
            │  ├─toml_parser-c0d25d6506131f06
            │  ├─toml_parser-e410d099613fc5c4
            │  ├─toml_writer-178ab88a089e24e0
            │  ├─toml_writer-eefdcae540d7affa
            │  ├─tracing-core-a7126bc5f3543fc1
            │  ├─tracing-f7d8b36a16a6070e
            │  ├─typeid-4ed44f4096d1cd87
            │  ├─typeid-59bd31bc53424343
            │  ├─typeid-b9b717f32433f7bf
            │  ├─typeid-cec6d6c3f0140491
            │  ├─typeid-fb38b6a3c7190df0
            │  ├─typenum-33ad54486525cd84
            │  ├─typenum-da5fe29de4576d53
            │  ├─typenum-de67bd99855e6c8e
            │  ├─unic-char-property-29a5b1664be23e83
            │  ├─unic-char-property-e4edcad669007114
            │  ├─unic-char-range-0651c1e4af9754fe
            │  ├─unic-char-range-97b854b34da92e94
            │  ├─unic-common-6167adb106cb3f44
            │  ├─unic-common-f3005df156be0991
            │  ├─unic-ucd-ident-03dbca7e58d2e1d5
            │  ├─unic-ucd-ident-7595ab7d1a0b9ccf
            │  ├─unic-ucd-version-84f8f33bd510d877
            │  ├─unic-ucd-version-d346d8e04fd34687
            │  ├─unicode-ident-bfa37b7ba93847f3
            │  ├─unicode-segmentation-07ed35c817c6ebe3
            │  ├─url-580f73db79a2293b
            │  ├─url-d537df09572bd1fd
            │  ├─urlpattern-6028900b5f82de13
            │  ├─urlpattern-f8cd5c1fc044ffa4
            │  ├─utf-8-3b0d31e7839eaf4f
            │  ├─utf8_iter-07fad3d88ddab750
            │  ├─utf8_iter-f5582a45b5971e92
            │  ├─uuid-dff5d89632494973
            │  ├─uuid-e0a364e08fdeefde
            │  ├─version_check-7d4c85e3faf10ac6
            │  ├─walkdir-6d0bd1472e2ab43e
            │  ├─walkdir-ca4a008f337b520f
            │  ├─webview2-com-56e82396c5d7a590
            │  ├─webview2-com-macros-1333931001ea9c9a
            │  ├─webview2-com-sys-47751580a1fce060
            │  ├─webview2-com-sys-8952a58e1c882c12
            │  ├─webview2-com-sys-b4a1f7f4720445f2
            │  ├─winapi-681539cdb918edec
            │  ├─winapi-81146fe0f255c42c
            │  ├─winapi-f3f53ca63e3621b8
            │  ├─winapi-util-677139e44fc36f04
            │  ├─winapi-util-fa92fb476e5406e5
            │  ├─winapi-x86_64-pc-windows-gnu-1204770e5e59e6cc
            │  ├─winapi-x86_64-pc-windows-gnu-13289adcdf643a74
            │  ├─winapi-x86_64-pc-windows-gnu-76d614a6e86a0b81
            │  ├─window-vibrancy-fd47c66f6165e395
            │  ├─windows-8a533abec9f9c8bc
            │  ├─windows-collections-c714a585e923d6cc
            │  ├─windows-core-e05a149fc14c9c22
            │  ├─windows-future-41d1bca8295fb738
            │  ├─windows-implement-611efcdb6509d39a
            │  ├─windows-interface-2cc31334af31dd44
            │  ├─windows-link-44192f00de015a1c
            │  ├─windows-link-4a2c67f34cb85bf4
            │  ├─windows-link-d8fe3b31c0ea9f5f
            │  ├─windows-numerics-975c05a5cd02ced1
            │  ├─windows-result-4974e032f687b7e6
            │  ├─windows-strings-26a9363176914790
            │  ├─windows-sys-1aec80eeebdb31c7
            │  ├─windows-sys-a8e264e484a4dae4
            │  ├─windows-sys-adfff2403dd06bd2
            │  ├─windows-sys-e840a9e72ccdcef4
            │  ├─windows-targets-15f42e6b58232733
            │  ├─windows-targets-76171b701ce14792
            │  ├─windows-threading-e32129ae89c2618c
            │  ├─windows-version-bb3b0b021117c420
            │  ├─windows_x86_64_gnu-2e281dc332048612
            │  ├─windows_x86_64_gnu-3e920f99d1ed1b11
            │  ├─windows_x86_64_gnu-4aa69f9f235b2fa0
            │  ├─windows_x86_64_gnu-5932e2848079ac9e
            │  ├─windows_x86_64_gnu-d3bede88b139fdf2
            │  ├─windows_x86_64_gnu-f0d6ae72b0adadba
            │  ├─winnow-c458b74330ad9b0a
            │  ├─winnow-f92cec2d28c9a4ee
            │  ├─writeable-62fbc4b8a16d5e12
            │  ├─writeable-fa9dd83e4a910dc4
            │  ├─wry-48a8c9c221770296
            │  ├─wry-668c59d145a6332f
            │  ├─wry-75bbe108de8fe01d
            │  ├─yoke-de2cbc19e2d2dab5
            │  ├─yoke-derive-bdea99f317a94911
            │  ├─yoke-f0e24ef43030cdc3
            │  ├─zerocopy-61ae61d720e9a9af
            │  ├─zerocopy-c23ab1473cf565d8
            │  ├─zerocopy-d531612050e6b2e7
            │  ├─zerofrom-628d839af885eff2
            │  ├─zerofrom-derive-f849c92f81b5a4b3
            │  ├─zerofrom-e3e09605e8f8e622
            │  ├─zerotrie-94feb56c08fa6178
            │  ├─zerotrie-de64bb424c977f8f
            │  ├─zerovec-8968eff92788504f
            │  ├─zerovec-bf9fbc2c9a87dc2a
            │  ├─zerovec-derive-9ad377e32515a711
            │  ├─zmij-31c2511b45cef231
            │  ├─zmij-72b6f09cafab563b
            │  ├─zmij-887f20316dcdcdb3
            │  ├─zmij-b0376e522cc31d6e
            │  └─zmij-ed50de01289abd5a
            ├─build
            │  ├─anyhow-1111dd729a3da0c8
            │  │  └─out
            │  ├─anyhow-f2bbce3567efcd3a
            │  │  └─out
            │  ├─anyhow-f73699a6bba4c3b5
            │  ├─camino-a899df70fd1ff3a4
            │  │  └─out
            │  ├─camino-b29df80ae03e5619
            │  ├─cookie-b44e032133bb9c05
            │  ├─cookie-be7d48e10fe14902
            │  │  └─out
            │  ├─crc32fast-a4e869e04d14a1f2
            │  │  └─out
            │  ├─crc32fast-c02bc35c2179f7e1
            │  ├─crossbeam-utils-177169e4a4e73c44
            │  ├─crossbeam-utils-29821470ac683c17
            │  │  └─out
            │  ├─cssparser-2ae4ceb791e83986
            │  │  └─out
            │  ├─cssparser-bd5383b13776bba5
            │  ├─erased-serde-5dc19c6d63c5191b
            │  ├─erased-serde-e2a6922ab5d87498
            │  │  └─out
            │  ├─erased-serde-ec93aa0ec963f067
            │  │  └─out
            │  ├─generic-array-0cd79ccd82607478
            │  │  └─out
            │  ├─generic-array-74e3286a7ae683ae
            │  ├─getrandom-0c8a9c83583f935d
            │  │  └─out
            │  ├─getrandom-1cc08b28c87f29ef
            │  ├─getrandom-7ad415ccbb72bb3c
            │  ├─getrandom-9b6e87f8f9d0b178
            │  │  └─out
            │  ├─getrandom-fd5676adc65be326
            │  │  └─out
            │  ├─icu_normalizer_data-3ebd831f38976c81
            │  ├─icu_normalizer_data-6b92c08fb3fb48ad
            │  │  └─out
            │  ├─icu_normalizer_data-cf76dd6e2cc031f7
            │  │  └─out
            │  ├─icu_properties_data-1c5a222da0fa1028
            │  │  └─out
            │  ├─icu_properties_data-539a6bc240d5f9aa
            │  ├─icu_properties_data-77e9bb239dcb8845
            │  │  └─out
            │  ├─indexmap-d20cf8c338dd2073
            │  │  └─out
            │  ├─indexmap-e758aca1ad43b735
            │  ├─libc-d533098e101f77ca
            │  ├─libc-d602a6fefe35e36d
            │  │  └─out
            │  ├─markup5ever-0fc6931d9bcd32e2
            │  ├─markup5ever-16d147e9df44d1c6
            │  │  └─out
            │  ├─parking_lot_core-05578ef725f00621
            │  ├─parking_lot_core-2d4d2aa69d66ea62
            │  │  └─out
            │  ├─parking_lot_core-3975e4ba3d592c63
            │  │  └─out
            │  ├─proc-macro-hack-3f9db64bdd7eeed2
            │  ├─proc-macro-hack-e28c4d3b50728336
            │  │  └─out
            │  ├─proc-macro2-641518f2893257a3
            │  │  └─out
            │  ├─proc-macro2-b8ba1343c081c770
            │  ├─quote-0f9325ac0e1f74fe
            │  ├─quote-c6a80555d744b6ba
            │  │  └─out
            │  ├─rfd-498561fb827f8530
            │  ├─rfd-f73c99fdafe2b245
            │  │  └─out
            │  ├─robsidian-b72987d865d63a37
            │  │  └─out
            │  │      ├─app-manifest
            │  │      └─tauri-codegen-assets
            │  ├─robsidian-fc896c8f829342d2
            │  ├─schemars-346be731d81b64d3
            │  ├─schemars-e46763b2f5c6cb0e
            │  │  └─out
            │  ├─selectors-23b11b9bb1300037
            │  ├─selectors-55cbd44eeb0fdb50
            │  │  └─out
            │  ├─serde-0c71ddf870e5bbea
            │  │  └─out
            │  ├─serde-3140e029bb1fd9ae
            │  ├─serde-49e0aad632408faa
            │  │  └─out
            │  ├─serde-cb064accfb94d26f
            │  ├─serde_core-9b489af8b7a09b15
            │  │  └─out
            │  ├─serde_core-b3aa795f2956ec6d
            │  ├─serde_core-cd6687b5c9e9302e
            │  ├─serde_core-dbf06a3af37b6c67
            │  │  └─out
            │  ├─serde_json-127e5a0e9d89e4e4
            │  ├─serde_json-2e019e996d1d130f
            │  │  └─out
            │  ├─serde_json-3980b81d2acc2f25
            │  │  └─out
            │  ├─serde_json-e69e69fad3e01d9f
            │  ├─syn-b391cc75aec4edbe
            │  │  └─out
            │  ├─syn-e2dbf8d5b0cabc91
            │  ├─tauri-1327a37cf5c10c5c
            │  ├─tauri-c40bbf537df5a54a
            │  │  └─out
            │  │      └─permissions
            │  │          ├─app
            │  │          │  └─autogenerated
            │  │          │      └─commands
            │  │          ├─event
            │  │          │  └─autogenerated
            │  │          │      └─commands
            │  │          ├─image
            │  │          │  └─autogenerated
            │  │          │      └─commands
            │  │          ├─menu
            │  │          │  └─autogenerated
            │  │          │      └─commands
            │  │          ├─path
            │  │          │  └─autogenerated
            │  │          │      └─commands
            │  │          ├─resources
            │  │          │  └─autogenerated
            │  │          │      └─commands
            │  │          ├─tray
            │  │          │  └─autogenerated
            │  │          │      └─commands
            │  │          ├─webview
            │  │          │  └─autogenerated
            │  │          │      └─commands
            │  │          └─window
            │  │              └─autogenerated
            │  │                  └─commands
            │  ├─tauri-plugin-dialog-b60b047a2d1b7843
            │  ├─tauri-plugin-dialog-ef79495d3dfc59bb
            │  │  └─out
            │  ├─tauri-plugin-fs-2a4d46c4cd2a69c8
            │  ├─tauri-plugin-fs-5933ebc79c2776a3
            │  │  └─out
            │  ├─tauri-plugin-opener-35d0a93b7f548171
            │  ├─tauri-plugin-opener-dd082f06fe1adfd3
            │  │  └─out
            │  ├─tauri-runtime-59a29b06d9633cc4
            │  │  └─out
            │  ├─tauri-runtime-f32d1faed27cb5b7
            │  ├─tauri-runtime-wry-249f448d012d18c3
            │  ├─tauri-runtime-wry-df0e1cd81588220e
            │  │  └─out
            │  ├─thiserror-137697f7faaac4a1
            │  │  └─out
            │  ├─thiserror-150d6a2088d5d817
            │  │  └─out
            │  ├─thiserror-274cd781366ec4b6
            │  ├─thiserror-6b3038345d51aefb
            │  │  └─out
            │  ├─thiserror-886ee717f1155ff1
            │  │  └─out
            │  ├─thiserror-b98ce535635ffd97
            │  ├─typeid-59bd31bc53424343
            │  │  └─out
            │  ├─typeid-b9b717f32433f7bf
            │  │  └─out
            │  ├─typeid-fb38b6a3c7190df0
            │  ├─typenum-33ad54486525cd84
            │  ├─typenum-da5fe29de4576d53
            │  │  └─out
            │  ├─webview2-com-sys-47751580a1fce060
            │  │  └─out
            │  │      ├─arm64
            │  │      ├─x64
            │  │      └─x86
            │  ├─webview2-com-sys-b4a1f7f4720445f2
            │  ├─winapi-681539cdb918edec
            │  │  └─out
            │  ├─winapi-f3f53ca63e3621b8
            │  ├─winapi-x86_64-pc-windows-gnu-1204770e5e59e6cc
            │  │  └─out
            │  ├─winapi-x86_64-pc-windows-gnu-13289adcdf643a74
            │  ├─windows_x86_64_gnu-4aa69f9f235b2fa0
            │  │  └─out
            │  ├─windows_x86_64_gnu-5932e2848079ac9e
            │  ├─windows_x86_64_gnu-d3bede88b139fdf2
            │  │  └─out
            │  ├─windows_x86_64_gnu-f0d6ae72b0adadba
            │  ├─wry-668c59d145a6332f
            │  │  └─out
            │  ├─wry-75bbe108de8fe01d
            │  ├─zerocopy-61ae61d720e9a9af
            │  ├─zerocopy-c23ab1473cf565d8
            │  │  └─out
            │  ├─zmij-72b6f09cafab563b
            │  │  └─out
            │  ├─zmij-887f20316dcdcdb3
            │  └─zmij-ed50de01289abd5a
            │      └─out
            ├─bundle
            │  ├─msi
            │  └─nsis
            ├─deps
            ├─examples
            ├─incremental
            ├─nsis
            │  └─x64
            ├─resources
            └─wix
                └─x64
```