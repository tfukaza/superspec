/**
 * Input allowances shared with Rust (`superspec_types::limits`); a Rust test keeps them identical.
 * Sized for real scenes: production bakes write a few 2048 px maps per material, and imported
 * scene GLBs are commonly 50-500 MB. Each value is a product of integers and `MIB`.
 */
const MIB = 1024 * 1024;

/** One model file (GLB) or other companion file. */
export const MAX_FILE_BYTES = 512 * MIB;
/** One encoded image (PNG, JPEG or KTX2), standalone or inside a model. */
export const MAX_IMAGE_BYTES = 64 * MIB;
/** All companion files of one source, project or scene, together. */
export const MAX_COMPANION_BYTES = 1024 * MIB;
/** Number of companion files of one source, project or scene. */
export const MAX_COMPANIONS = 1024;
/** Encoded images delivered in one scene packet. */
export const MAX_SCENE_IMAGE_BYTES = 1024 * MIB;
/** A saved project file, whose companions are embedded as base64 (4 bytes per 3), plus 64 MiB. */
export const MAX_PROJECT_FILE_BYTES = 1_498_764_628;

export const MAX_SCENE_IMAGES = 16_384;
export const MAX_BATCHES = 16_384;
export const MAX_INSTANCES = 1_000_000;
export const MAX_BATCH_VERTICES = 6_000_000;
export const MAX_BATCH_INDICES = 36_000_000;
export const MAX_BATCH_ATTRIBUTE_FLOATS = 64_000_000;
export const MAX_PROJECT_MODELS = 16;
export const MAX_PROJECT_MATERIAL_ASSIGNMENTS = 64;
export const MAX_MATERIAL_LOD_ALTERNATIVES = 16;
export const MAX_GEOMETRY_LOD_LEVELS = 16;
export const MAX_GEOMETRY_LOD_GROUPS = 16_384;

/** A short human-readable size for limit messages. */
export function megabytes(bytes: number): string {
  return `${Math.round(bytes / MIB)} MB`;
}
