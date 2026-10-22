# Local AAC PNS Patch

Upstream: `symphonia-codec-aac 0.6.0`, from the crates.io archive with SHA-256 `f1979c515a76371b186aad2feff5f23e21cbec775bf95de08bf1e3af92a2ad76`.

Recorded upstream revision: [980bf5830a90e069fd64641d9c38f067ab772a24](https://github.com/pdeljanov/Symphonia/tree/980bf5830a90e069fd64641d9c38f067ab772a24/symphonia-codec-aac). The imported `Cargo.toml`, `src/`, `tests/`, `README.md`, and MPL-2.0 `LICENSE` come from that checksum-verified archive. Upstream copyright and license headers remain intact.

The maintainer approved this scoped patch on 2026-10-22. Two source files differ:

- `src/aac/mod.rs` passes each encoded packet's signed presentation timestamp to channel-pair setup.
- `src/aac/cpe.rs` seeds the existing Numerical Recipes LCG from that timestamp and channel position before decoding a packet. New and reused channel pairs follow the same path. The high timestamp bits participate so ordinary low-word wrap does not repeat the same noise window.

Upstream retains the PNS generator across packets and does not restore it on reset. Its noise draws depend on earlier coded bands, so fixed preroll cannot reproduce sequential PCM. Packet-local seeding removes that history dependency without replaying the file, caching PCM, changing band energy scaling, adding dependencies, or changing the decoder API. Noise-substituted PCM changes from the upstream implementation; non-PNS coefficient decoding and the overlap/window implementation are unchanged. PNS synthesis still uses the existing generator and normalization, with bounded allocation-free seed setup outside the output callback.

Regression coverage lives in `crates/stemplayer-decode/tests/aac_pns.rs`: repeated reset/seek, sequential-versus-seek with two-packet preroll, signed/large timestamps, and differing positions/channels. The two original demonstrations failed against the registry crate and now run without `ignore`.

Distribution must include the MPL terms and identify the modified source at the exact public source commit recorded in `BUILD-INFO.json`. Do not distribute while that source location is private or unavailable. The source is under this repository's `vendor/symphonia-codec-aac/` directory; registry download URLs describe only the unmodified upstream crate.

Remove this patch when a reviewed upstream release provides deterministic bounded-seek PNS behavior and these regressions pass without local changes. Listening, broader codec conformance, and release qualification remain separate gates.
