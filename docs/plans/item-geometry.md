# Item geometry ownership

ItemSpec owns bounded procedural mesh storage, vertex admission, surface slots,
UV binding and plain polygon emission. These operations have no dependency on a
theme, AML, asset lookup or material recipes. Material mapping uses MaterialSpec's
layout contract, and completed meshes implement the shared model's visual input.

Kowloon retains its weathering collection and finish-aware Pen. Its mesh wrapper
contains the ItemSpec mesh, with dereferencing for existing callers. The explicit
owned/bind adapter carries theme planning state and its identity across workers;
the generic mesh has neither. No geometry is copied by that adapter.

Plain face emission receives the transform, part, local points and mapping inputs.
A color callback receives local position, world position and final corner normal.
The theme evaluates tint then shading at exactly the existing call sites. Survey
mode and degenerate triangles do not evaluate color. Existing attempted-work,
triangle admission, rounding, winding, partial failure and cancellation behavior
remain unchanged. Composite theme operations continue to dispatch their finish
hooks before reaching this common emission path.

Public ItemSpec primitives check finite orthonormal frames, including reflections,
using the existing Pen constructor contract. Theme adapters keep the original
validation points through crate-private admitted emission, including legacy
composed-frame tolerances. This preserves theme behavior while preventing new
callers from silently receiving incorrect normals from scaled or skewed frames.

Retain the seven existing `pen_emission` tests through the theme
adapter, and add a direct ItemSpec consumer test. Compare complete packets and
exports with the frozen executable and retain existing receiver and worker tests.
