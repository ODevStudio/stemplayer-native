# Feature Matrix

| Target | Supported feature set |
| --- | --- |
| Domain and pure contract crates | No default features |
| `stemplayer-probe` minimal | No features |
| `stemplayer-probe` formats | `local-wav,local-mp4,aac,alac,hls` |
| `stemplayer` Windows product | `product` |
| Tests | `fake-decoder,fake-output` where the engine boundary is exercised |

Features isolate implementations and never change domain type semantics. macOS and Linux integration features compile as empty Phase 1 boundaries; runtime support remains Phase 12 work.
