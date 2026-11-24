# Deferred native hooks

These source-only copies are based on crates.io wgpu and wgpu-core **30.0.1**.
Original package SHA-256 checksums:

- wgpu: `527ccdf43dd5b2e8676eed9984ce00e2bbb0a1b85b70c1969dcb6cd2eb55ab9e`
- wgpu-core: `14c018fce9b6270aa203c2fdd56f3cce996713534bd757e4ea58c8560b121f14`

The MIT and Apache-2.0 licenses are included in both directories. Source, normalized
manifests, build scripts and readmes are retained; registry metadata and build
artifacts are excluded.

The opt-in `native-hooks` feature adds an unsafe recording seam for native work
that must run among deferred commands. It does **not** disable `EncodingApi`'s
prohibition on mixing deferred high-level and immediate raw encoding.

`Command::Native` and `RenderCommand::Native` retain an `Arc<dyn NativeCommand>`.
Recording uses normal encoder/pass state checks. Finish executes callbacks in
stream order. Render hooks reset recording deduplication and replay binding state
like render bundles: callers must restore pipeline, vertex/index buffers and bind
groups before ordinary draws. The native callback must preserve the enclosing
pass, viewport, scissor and unrelated dynamic state. Pipeline raster/depth state
may change and is restored by the next ordinary pipeline binding.

Before invoking a callback, core also retains its Arc in command-buffer temporary
resources. Those move into the active submission and retire only after the GPU
fence. Dropping an unfinished or unsubmitted command stream releases its ownership
normally. Error paths retain already-encoded work in the same way.

External resource identity, initialization, compatible formats, usage, bounds and
hazard synchronization remain the unsafe caller's responsibility. Callbacks and
their destructors must not re-enter wgpu while its device snatch guard is held.
Use retained immutable native/HAL handles, not frontend wgpu resource wrappers or
mutable CPU argument encoders. Do not retain references to the borrowed encoder.

Tracing rejects native recording because native commands cannot be serialized;
serde explicitly skips the variants, and trace conversion cannot silently omit
one. Normal behavior with this feature disabled remains upstream behavior.

Changes are confined to the two manifests; wgpu's command/render API forwarding;
core's encoder/render IR and replay, temporary-resource ownership and trace
conversion; and render-bundle exhaustive matches (which reject native hooks).
The Metal HAL exposes borrowed active encoder/buffer/view handles and a helper
that closes implicit copy/AS encoders before standalone external work.

`horizon-metal-terrain` tests exercise deferred ordering, abandonment, fence
retirement, same-pipeline restoration after hooks, actual GPU ICB reset/culling,
packed-vertex boundary decoding, and opaque/entity/translucent color/depth order
with producer teardown before submission. The managed reference gate requires the
Metal regressions and live native submission marker.
