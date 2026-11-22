import { execFileSync } from "node:child_process";
import { mkdtemp, open, readFile, rm, rmdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** Run a binary protocol without pipes on hosts that restrict named pipes. */
export async function runNativeBinary(executable, args, request) {
  const directory = await mkdtemp(join(tmpdir(), "superspec-stdio-"));
  const input = join(directory, "input"), output = join(directory, "output");
  try {
    if (request !== undefined) await writeFile(input, request);
    await using inputFile = request === undefined ? undefined : await open(input, "r");
    await using outputFile = await open(output, "wx");
    execFileSync(executable, args, { windowsHide: true, stdio: [inputFile?.fd ?? 0, outputFile.fd, 2] });
    return await readFile(output);
  } finally {
    await rm(input, { force: true });
    await rm(output, { force: true });
    await rmdir(directory);
  }
}
