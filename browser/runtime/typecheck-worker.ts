/**
 * Strict TypeScript over a virtual file system: the generated SDK declarations, the project's
 * `.material.ts` modules and the ES5 library. Never emits and never executes a recipe.
 */
import ts from "material-typescript";
import es5 from "../../node_modules/material-typescript/lib/lib.es5.d.ts?raw";
import decorators from "../../node_modules/material-typescript/lib/lib.decorators.d.ts?raw";
import legacyDecorators from "../../node_modules/material-typescript/lib/lib.decorators.legacy.d.ts?raw";
import type { TypeCheckRequest, TypeCheckResult } from "./typecheck.ts";

const SDK_PATH = "/sdk/index.d.ts";
const LIB_PATH = "/lib/lib.es5.d.ts";
const options: ts.CompilerOptions = {
  strict: true, noEmit: true, target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext,
  moduleResolution: ts.ModuleResolutionKind.Bundler, types: [], allowImportingTsExtensions: true,
  lib: ["lib.es5.d.ts"], baseUrl: "/", paths: { "@superspec/material/v1": [SDK_PATH] },
};
let previous: ts.Program | undefined;

self.onmessage = (event: MessageEvent<TypeCheckRequest>) => {
  const { requestId, sdk, modules } = event.data;
  try {
    // lib.es5.d.ts references the decorator declarations.
    const files = new Map<string, string>([[SDK_PATH, sdk], [LIB_PATH, es5],
      ["/lib/lib.decorators.d.ts", decorators], ["/lib/lib.decorators.legacy.d.ts", legacyDecorators]]);
    const roots: string[] = [];
    for (const [name, source] of Object.entries(modules)) {
      if (!name.endsWith(".material.ts")) continue;
      const path = `/src/${name}`;
      files.set(path, source);
      roots.push(path);
    }
    const host: ts.CompilerHost = {
      fileExists: path => files.has(path),
      readFile: path => files.get(path),
      getSourceFile: (path, language) => {
        const text = files.get(path);
        return text === undefined ? undefined : ts.createSourceFile(path, text, language, true);
      },
      getDefaultLibFileName: () => LIB_PATH,
      getDefaultLibLocation: () => "/lib",
      writeFile: () => undefined,
      getCurrentDirectory: () => "/",
      getCanonicalFileName: path => path,
      useCaseSensitiveFileNames: () => true,
      getNewLine: () => "\n",
      directoryExists: path => [...files.keys()].some(file => file.startsWith(path.endsWith("/") ? path : `${path}/`)),
      getDirectories: () => [],
    };
    const program = ts.createProgram({ rootNames: [SDK_PATH, ...roots], options, host, oldProgram: previous });
    previous = program;
    const findings = ts.getPreEmitDiagnostics(program).map(diagnostic => {
      const file = diagnostic.file;
      const start = diagnostic.start ?? 0;
      const position = file ? file.getLineAndCharacterOfPosition(start) : { line: 0, character: 0 };
      return {
        sourceName: file?.fileName.startsWith("/src/") ? file.fileName.slice(5) : file?.fileName,
        line: position.line + 1, column: position.character + 1, offset: start, end: start + (diagnostic.length ?? 0),
        code: `TS${diagnostic.code}`, message: ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n"),
      };
    });
    self.postMessage({ requestId, findings } satisfies TypeCheckResult);
  } catch (error) {
    self.postMessage({ requestId, findings: [], error: error instanceof Error ? error.message : String(error) } satisfies TypeCheckResult);
  }
};
