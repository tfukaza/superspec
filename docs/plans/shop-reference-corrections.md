# Shop reference corrections

> Superseded on 2026-11-22: the frozen JSON captures, their maintenance drivers and the tests that compared generator output against them were removed. Tests now check behaviour rather than captured outcomes; this record is historical.

The extraction retained the Rust implementation at `fb5df8c`. Its storefront
updates intentionally ported selected parts of the later TypeScript implementation.
The older frozen references must therefore be corrected by supported feature, not
replaced wholesale with either current Rust output or the complete later TypeScript
scene.

## Material roles and host inputs

The twelve original `kit-materials` cases were reproduced exactly using the
TypeScript kit and wall material binders at `14ae2704d66f1eaacf0cf24d17ad4e5db8b45839`.
The comparison includes complete bindings and definitions; only the product asset
scheme is normalized to `superspec-material:`.

The corrected reference applies the five semantic roles added in Rust `7b1ceab`:
`kitchen-metal`, `vent-metal`, `vent-edge`, `shop-register`, and `market-trays`.
Their definitions come from the independently authored TypeScript catalog at
`3c64de86ebc4390ccfb9ff31a29d0f840716b69a`, as do the restaurant-food roughness and
tarp repeat/metalness changes. Wall tile grime uses that catalog's intrinsic PBR
definition, which Rust adopted in `0f45f9c`. Authored declarations and all other
definitions remain unchanged. The later TypeScript ceramic and tile kit slots are
not added because that part was not ported.

The household test host omitted the new kitchen-metal role. Its adapter now supplies
the same catalog material and three embedded companion images that production
supplies. The complete delivery, asset binding and unchanged-collision assertions
remain in place. The material suite passes all three tests, and the household suite
passes its complete nineteen-builder case.

## Grocer reservations

All forty-eight original grocer layout plans and their exact RNG continuation values
were reproduced with the original TypeScript planner. The maintained Rust port
adopts the later planner's counter, cooler and shelving reservations through the
right-hand rack, while retaining the original crate, sack, stool and hanging
placements. The corrected reference combines those independently authored source
sections. It retains all forty-eight room geometry references and their inputs.

The newer planner admits side counters only when width is at least 3.8 meters and
depth is at least 4.2 meters. The old cases were all smaller. Eight additional
reference cases exercise that threshold and larger shelving arrangements, preserving
the existing requirement that all three layout kinds are covered. Both shop-room
tests pass, including invalid-input stream preservation.

The ignored evidence is under `generated/verification/sign-reference`: the reference
drivers, preserved old files, corrected outputs, source hashes, and raw focused test
logs. Each maintained fixture records the full source revisions and supported delta.
Normal builds do not require the historical TypeScript tree or the LevelSpec harness.

## Native composition contracts

The maintained Rust grocer, greengrocer and roast assemblies combine later planning
and stock arrangements with retained earlier subcomponents. No single historical
TypeScript revision describes those complete assemblies. Their old unused mesh
arrays have therefore been removed, with the original files and SHA-256 recorded
in verification evidence. Authored dimensions, matrices, seeds and detail cases
remain. The grocer also retains independently generated layout expectations.

These cases check complete render/survey position parity, finite material-complete
attributes, reflection and rigid placement, same-seed repeatability, changed-seed
stock, floor support, independent shelf and register dimensions, dining support,
and the deep market's clear central aisle. Exact independent furniture, stock,
restaurant, window and frontage component tests remain separate.
The roast oven pipe also retains its original complete 32-triangle geometry and
sampled receiver chart for every original case, selected by its authored cylinder
bounds from the finished native assembly.

The shell integration still compares every original clearance callback and sign
proposal. Its five enclosure planes require independent per-plane area, inward
normal and corner coverage. Openings and frontage retain complete independently
reproduced position, normal, UV, color and slot expectations. The original 22
opening receiver charts per case retain all 44 triangles and sampled values after
exact triangle mapping; frontage's expected absence of receivers is checked too.
This keeps registration errors detectable despite different interior triangle counts.

Connected AML references apply only exact independently compiled component deltas:
the reviewed room-depth reservation, window folds, and frontage cardboard profile.
Unrelated original seam subdivisions remain byte-preserved. Native trade interiors
are checked in their independently planned rooms, including support and material
closure. The tiled floor retains its complete expected triangles with the explicit
native `room-floor` ownership and must bind a raster material. Exterior envelopes,
openings, frontage, signs and connecting spans retain full triangle comparisons.

The maintenance drivers (since removed) lived beside the captures. They took
a reconstructed source/evidence directory. `building-shop-profile.mjs` applies the
reviewed subset to an unmodified historical tree; its street, shop and window
outputs were reproduced exactly against the source used for the corrected AML
captures. These scripts never use Rust output to construct expected geometry.

Focused validation passed the five previously failing building AML tests in
separate processes, all six native composition tests, the restored shell receiver
case and both roast tests with the retained pipe chart. The twelve kit cases,
forty-eight original room layouts plus eight admission cases, and six grocer
layouts were reproduced again by the maintained reference drivers. Raw logs are
under `generated/verification/sign-reference`; full workspace acceptance is tracked
separately.
