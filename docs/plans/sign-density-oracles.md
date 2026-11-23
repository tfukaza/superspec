# Sign-density reference correction

> Superseded on 2026-11-22: the frozen JSON captures, their maintenance drivers and the tests that compared generator output against them were removed. Tests now check behaviour rather than captured outcomes; this record is historical.

The five failing `building_aml` cases were stale reference data, not a new
placement or collision regression. The snapshots were introduced by LevelSpec
`af7f5e22d78f1f45ff7e5ba21c56c3097f12d13d`. The subsequent TypeScript change
`3c64de86ebc4390ccfb9ff31a29d0f840716b69a` deliberately reduced projecting-sign
probabilities from 0.62/0.42 to 0.48/0.16 for the first/upper stories, and wall
plates from 0.22 to 0.16. Merge
`7b1ceab13be2a95b4f125a12d4d7cc5dc8c6cfb6` ported those values to Rust without
updating the building snapshots.

The seeded choice for `connected-building/host/lot-0/side-0/sign-1-0?` is
0.22444316907785833. Low detail multiplies the probability by 0.4: that roll
was below the old 0.248 cutoff but is above the current 0.192 cutoff. The
proposal remains planned and passes the survey; its correct low-detail status
is `withheld`, with zero emitted triangles. The documented distinction between
detail withholding and spatial obstruction remains unchanged.

## Independent derivation

The historical TypeScript compiler at
`14ae2704d66f1eaacf0cf24d17ad4e5db8b45839` was reconstructed in ignored
verification storage. Its ordinary `parseAml` and `compile` functions consumed
the unchanged AML fixtures and shipped material assets. The driver collected
all generated triangle positions, normals and owners, plus the complete sign
plans. It did not read Rust output to construct expected geometry.

Eight single-host inputs reproduced every original triangle exactly. Empty
sign-plan output is normalized to an empty list; source hashes use LF text.
The two multi-host cases reproduced their sign plans and all non-sign geometry,
but the old TypeScript output has 44 fewer sign seam triangles than the frozen
oracle. Those unrelated subdivisions are deliberately retained.

Only the three reviewed probability changes were applied to that historical
source. The single-host reference then removed exactly 1,142 lightbox triangles
(1,126 emitted triangles plus 16 receiver-edge subdivisions), added none, and
changed only that sign's status and triangle count. The identical removal
multiset exists exactly in both multi-host frozen oracles. Their correction
therefore removes that independently established multiset and preserves every
other original triangle, including the differing fascia subdivisions. The
transformed base-wall case uses its separately reproduced transformed delta.

Each corrected fixture records the source and change revisions, patched source
hash, unchanged AML input, original fixture hash and delta input. Existing
complete geometry, collision, visibility, source-ownership and recovery checks
remain enabled. This correction does not change production generation.

The related `sign_street` domain oracle also predated the density change. All 36
original cases were reproduced exactly using the historical `Kit`, `Pen`,
`KitMesh`, `streetSigns` and `wallAdvertisement` implementations before applying
the same probability patch. Updated queries, nearby-shop lookups and complete
proposals come from that independent reference. Facade inputs, material controls,
detail levels, reflections, obstruction scenarios and independent random-stream
samples are preserved. The Rust tests still compare every field.

Local evidence is under `generated/verification/sign-reference/`: original and
corrected snapshots, the historical compiler driver, density-only source patch,
`baseline-comparison.json`, and `correction-report.json`. An independent reviewer
repeated the multiset proof directly against the original Git fixtures in
`generated/verification/review-sign-delta.json`. Normal Rust tests require none
of the historical TypeScript source or its dependencies.

## Standalone building survey

The full workspace run also exposed older plans in the eight-case `building`
fixture. The independent driver uses the original `KowloonBuildings.drawSite`
and `SignBook`, with the fixture's neutral material bindings and empty obstruction
survey. It reproduces every original plan and every sign triangle's position,
normal, UV, color and material slot, plus all 2,991 original receiver samples.
Frame receivers project delivered positions into their declared source frame;
raw primitive UVs alone are not the admitted receiver coordinates.

The reviewed density change removes one withheld proposal in each of cases two
and three. In the tall `branch-0` case it removes twelve proposals and withholds
four formerly built signs. Three retained upper signs move when lower reservations
disappear. The corrected expectation therefore uses the full independent survey,
including moved geometry and sampled receivers. It preserves the current fixture's
window, room and canvas corrections and every unchanged sign record. UV and
receiver comparisons remain strict.

The maintained `building-signs-setup`, `building-signs-capture`,
`building-signs-samples` and `building-signs-correct` scripts (since removed) accepted explicit source, original-fixture and output
paths. The final corrector records the current fixture hash and writes a separate
candidate; publication remains a deliberate merge step.
