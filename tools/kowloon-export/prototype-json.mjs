import { open } from "node:fs/promises";

/** Receiver discovery and LOD detection need the JSON chunk, even on prototypes with large embedded maps. */
export async function readPrototypeJson(source, maxJsonBytes = Infinity) {
  const file = await open(source, "r");
  try {
    const read = async (length, offset) => {
      const bytes = Buffer.alloc(length);
      for (let at = 0; at < length;) {
        const { bytesRead } = await file.read(bytes, at, length - at, offset + at);
        if (!bytesRead) throw Error("Truncated prototype GLB: " + source);
        at += bytesRead;
      }
      return bytes;
    };
    const header = await read(20, 0);
    if (header.toString("ascii", 0, 4) !== "glTF"
      || header.readUInt32LE(4) !== 2 || header.toString("ascii", 16, 20) !== "JSON"
      || header.readUInt32LE(8) !== (await file.stat()).size) throw Error("Invalid prototype GLB: " + source);
    const length = header.readUInt32LE(12);
    if (!length || length % 4 || length > maxJsonBytes || length + 20 > header.readUInt32LE(8))
      throw Error("Invalid prototype JSON size: " + source);
    return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(await read(length, 20)));
  } finally { await file.close(); }
}
