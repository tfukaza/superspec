# Image admission ownership

The compiler session orchestrates source compilation and owns the lifetime of
reused artifacts. Image decoding/admission and its bounded metadata cache move to
a private `assets::image_admission` module. Architecture depends on that admission contract
instead of reaching into the session implementation for image types and decoding.

Keep exact-byte identity, limits, eviction order, counters, failure behavior and
public `CacheStats` unchanged. This is an ownership move; it introduces no decoded
image retention or other changes to reuse. Existing session-cache behavior tests
and complete output comparisons cover the extraction.
