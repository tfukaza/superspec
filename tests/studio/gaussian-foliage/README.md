# TreeSpec Gaussian foliage

A Three.js comparison for a small concrete patch, grass, and a shrub. Drag to orbit or enable
**Auto-orbit**, then switch between **3D splats** and **Camera-facing dabs**. The modes use the
same TreeSpec blades and leaf cards, colors, and depth order. **Density** thins both in place,
and the two checkboxes isolate each kind of foliage. Drag **Edge firmness** left for the
plain Gaussian falloff or right for a compact dab with a firmer edge.
At the firm end, leaves mix clipped flat strokes, tapered strokes, and strokes
with shallow bristle cuts. Each mark's profile follows its projected long axis.
Nearby leaf dabs rotate by individual amounts within their leaf planes.
**Leaf normal variation** tilts the shading normals of individual leaf dabs
by up to the selected angle, 24° by default. Their tilts are stable as the sun
and camera move.
**Pigment patches** makes nearby leaves share a color in plant space and grass
clumps share color across the ground. Pigment cells are roughly 16–19 cm across on
leaves and 34–39 cm on grass. Per-dab variation remains inside
each patch. The larger marks keep their positions when you move the camera or
sun, and the brush texture comes from overlapping leaves and blades.
**Leaf hue variation** mixes a smooth three-dimensional hue field with a
stable warm or cool shift per leaf. The field also colors the distant canopy shell.
**Interior fill** adds a sparse layer of dark, round dabs in dense parts of the
canopy. A dab's perimeter must be supported by foliage density. The two nearest
detail levels draw these dabs in the same sorted pass as the leaf strokes.

Use **Scene** to view the original TreeSpec birch and poplar separately or
together. The presets retain the trees' branching and crown shapes and tune the
leaf cards and surface-dab size for the splat renderer. **Canopy close-up** moves
the camera into the nearest detail band, and **Reset view** returns to a framing
that fits the entire tree.

From the repo root, with `superspec-studio` on PATH or its executable path in `STUDIO_CLI`:

```sh
node tests/studio/gaussian-foliage/build.mjs
npx vite --config tests/studio/gaussian-foliage/vite.config.mjs
```

The build runs the ordinary TreeSpec `scene` command over the shrub, meadow,
birch and poplar fixtures, with demo-specific settings and a small grass site.
The page decodes the selected scene's packets and fits one oriented Gaussian to
each leaf card or grass blade used by the demo.
The browser projects the three axes into a screen-space ellipse, sorts dabs by
camera depth when the view direction changes, and draws the foliage in one
instanced transparent pass. The comparison projects a camera-facing ellipse of the
same authored size. Bark uses the ordinary opaque TreeSpec mesh.
The covariance projection follows the formulation in the
[original Gaussian splatting paper](https://repo-sam.inria.fr/fungraph/3d-gaussian-splatting/).

## Scale and lighting

The larger field options in **Scene** repeat the same TreeSpec patch 16 or 64 times without regenerating its
foliage. **Detail** switches between submitting everything, frustum culling, and
culling with four distance bands. The bands start at 0, 9, 17, and 31 meters
from each patch's bounds. They retain 100%, 42%, 14%, and 4.5% of the dabs and
widen the survivors. These bands are specific to this demo. The selection steps
are visible at some distances and the renderer still sorts all surviving dabs together.

The tall trees select each tree's authored TreeSpec LOD independently. Within
roughly 6 meters of its bounds, a tree adds a tier of fine twigs, denser and
smaller leaf dabs, and tiny fans of dabs on selected clump surfaces. Some
clumps stretch along their dominant axis and narrow across it. The same
reshaping applies to the distant canopy shells.
From 6 meters the original branch hierarchy carries fewer, wider dabs. From 22
meters the main limbs carry a lower-density canopy with shell backing. From 45
meters the trunk, a coarse canopy shell, and a few broad dabs supply the silhouette. The
readout shows each tree's current level, and **Submit everything** forces the
close detail for comparison. The preview switches at the authored thresholds.
The cached branch and canopy shadow proxies stay fixed across visible LODs.

**Live lighting** shades grass by blade orientation. Leaves sample a virtual
normal field built from the close foliage, with TreeSpec's clump surface normals
for mass dabs. The tree presets group leaves on smaller branches. Two scales
of directional density variation leave irregular gaps in mass dabs, and the
native sprig leaves draw through those openings. Shorter leaf dabs and soft,
low-opacity interior fills leave the branch silhouettes visible. Local normal
bands describe the sprays, and sun-relative position darkens the back of the crown. TreeSpec
openness gives buried leaves less ambient light. The shared canopy ramp runs
from deep blue-green through green to muted golden olive in direct sun.
Turning the sun
updates lighting uniforms. The shadow map uses the TreeSpec canopy proxy, bark, and coarse
grass ribbons. The concrete and ground receive conventional shadows, and each dab
samples the same map at its center. A stationary sun reuses the map as the camera
moves. The larger fields cast detailed foliage shadows in the central 16-meter
shadow region. Proxy silhouettes are approximate and stay at their own density
when the visible dabs thin.

For a bundled preview, run `npx vite build --config tests/studio/gaussian-foliage/vite.config.mjs`.
Serve `generated/treespec-gaussian-demo` over HTTP.

After bundling, run `node tests/studio/gaussian-foliage/bench.mjs` to compare a
wide orbit and a low moving view. The benchmark records active dabs, main-thread
selection, sort, packing and submission, GPU frame time where timer queries are
available, and frame interval. It saves data and screenshots to
`generated/treespec-gaussian-foliage`. On Windows, set
`BROWSER_ARGS=--use-angle=d3d11 --ignore-gpu-blocklist` to measure on the GPU.
The live frame-rate readout is capped by display refresh. The benchmark separates
cached shadows from a sun that moves every frame.
