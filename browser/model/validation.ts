export interface NavigationMesh { vertices: number[][]; polygons: number[][]; components: number[] }
export interface FindingDiagnostic { severity: string; code: string; message: string; objects: string[]; source: { line: number; column: number; offset: number; end?: number }; spatial?: { bounds: number[][]; inspectionTarget: string; evidence: string } }
export interface ReviewFinding extends FindingDiagnostic { id: string; fingerprint: string; state: string; suppression?: { reason: string; stale: boolean } }
/** Physical findings and movement-specific navigation evidence produced by Rust. */
export interface ValidationReport {
  success: boolean;
  error?: string;
  geometryStatus?: string;
  navigationStatus?: string;
  diagnostics: FindingDiagnostic[];
  reviews?: ReviewFinding[];
  navigation?: NavigationMesh;
  water?: { navigation: NavigationMesh; boat: { radius: number; height: number; draft: number } };
  routes?: { id: string; reachable: boolean; distance: number | null; path: number[][]; verifiedRoutes: number; requestedRoutes: number; maximumRoutes: number | null; traversals: string[] }[];
}
