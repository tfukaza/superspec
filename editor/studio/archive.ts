/**
 * Studio project archives (`.studio.zip`): a ZIP file holding `project.json` and one entry per
 * companion under `assets/`. JSON projects embed companions as base64 in a single string, which
 * JavaScript caps at about 512 MB of text, so large projects are saved as archives instead.
 * `project.json` is the same document as a JSON project, except that each `assets` value names
 * its archive entry instead of holding base64.
 *
 * Archives are written uncompressed (images and GLBs are already compressed) and deterministically.
 * Stored and deflated entries are read, so an archive re-zipped by another tool still opens. ZIP64
 * is not needed: the project file limit is below 4 GiB.
 */
export const PROJECT_ENTRY = "project.json";
export const ASSET_DIRECTORY = "assets/";
const LOCAL = 0x04034b50, CENTRAL = 0x02014b50, END = 0x06054b50;
const UTF8 = 0x0800;
/** 1980-01-01 00:00, the earliest DOS date, so the same project always gives the same bytes. */
const DOS_TIME = 0, DOS_DATE = (1 << 5) | 1;

const table = (() => {
  const values = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    values[n] = c >>> 0;
  }
  return values;
})();

/** CRC-32 (ISO 3309), as ZIP stores it. */
export function crc32(bytes: Uint8Array): number {
  let c = 0xffffffff;
  for (let i = 0; i < bytes.length; i++) c = table[(c ^ bytes[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

/** Writes an uncompressed ZIP; entries keep their given order. */
export function writeArchive(entries: { name: string; bytes: Uint8Array }[]): Blob {
  const parts: BlobPart[] = [];
  const directory: Uint8Array<ArrayBuffer>[] = [];
  let offset = 0;
  for (const { name, bytes } of entries) {
    const encoded = new TextEncoder().encode(name);
    const crc = crc32(bytes);
    const local = new DataView(new ArrayBuffer(30));
    local.setUint32(0, LOCAL, true); local.setUint16(4, 20, true); local.setUint16(6, UTF8, true);
    local.setUint16(8, 0, true); local.setUint16(10, DOS_TIME, true); local.setUint16(12, DOS_DATE, true);
    local.setUint32(14, crc, true); local.setUint32(18, bytes.length, true); local.setUint32(22, bytes.length, true);
    local.setUint16(26, encoded.length, true); local.setUint16(28, 0, true);
    const central = new DataView(new ArrayBuffer(46 + encoded.length));
    central.setUint32(0, CENTRAL, true); central.setUint16(4, 20, true); central.setUint16(6, 20, true);
    central.setUint16(8, UTF8, true); central.setUint16(10, 0, true); central.setUint16(12, DOS_TIME, true);
    central.setUint16(14, DOS_DATE, true); central.setUint32(16, crc, true); central.setUint32(20, bytes.length, true);
    central.setUint32(24, bytes.length, true); central.setUint16(28, encoded.length, true);
    central.setUint32(42, offset, true);
    new Uint8Array(central.buffer).set(encoded, 46);
    parts.push(local.buffer, encoded, bytes as Uint8Array<ArrayBuffer>);
    directory.push(new Uint8Array(central.buffer));
    offset += 30 + encoded.length + bytes.length;
  }
  const size = directory.reduce((sum, entry) => sum + entry.length, 0);
  if (offset + size > 0xffffffff || entries.length > 0xffff) throw new Error("Project archive exceeds the ZIP size limits.");
  const end = new DataView(new ArrayBuffer(22));
  end.setUint32(0, END, true); end.setUint16(8, entries.length, true); end.setUint16(10, entries.length, true);
  end.setUint32(12, size, true); end.setUint32(16, offset, true);
  return new Blob([...parts, ...directory, end.buffer], { type: "application/zip" });
}

/** True for bytes that start like a ZIP file. */
export function isArchive(bytes: Uint8Array): boolean {
  return bytes.length >= 4 && new DataView(bytes.buffer, bytes.byteOffset, 4).getUint32(0, true) === LOCAL;
}

async function inflate(bytes: Uint8Array, size: number): Promise<Uint8Array> {
  const stream = new Blob([bytes as BlobPart]).stream().pipeThrough(new DecompressionStream("deflate-raw"));
  const output = new Uint8Array(size);
  let length = 0;
  for await (const chunk of stream as unknown as AsyncIterable<Uint8Array>) {
    if (length + chunk.length > size) throw new Error("A project archive entry is larger than it declares.");
    output.set(chunk, length);
    length += chunk.length;
  }
  if (length !== size) throw new Error("A project archive entry is smaller than it declares.");
  return output;
}

/**
 * Reads every entry of a ZIP file. `limit(name, size)` sees each declared size before any bytes are
 * read or inflated and throws to reject the archive.
 */
export async function readArchive(bytes: Uint8Array, limit: (name: string, size: number) => void): Promise<Map<string, Uint8Array>> {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let end = -1;
  for (let at = bytes.length - 22; at >= Math.max(0, bytes.length - 22 - 0xffff); at--) {
    if (view.getUint32(at, true) === END) { end = at; break; }
  }
  if (end < 0) throw new Error("This is not a ZIP project archive.");
  const count = view.getUint16(end + 10, true);
  let at = view.getUint32(end + 16, true);
  const entries = new Map<string, Uint8Array>();
  for (let index = 0; index < count; index++) {
    if (at + 46 > bytes.length || view.getUint32(at, true) !== CENTRAL) throw new Error("Corrupt project archive directory.");
    const flags = view.getUint16(at + 8, true), method = view.getUint16(at + 10, true);
    const crc = view.getUint32(at + 16, true), compressed = view.getUint32(at + 20, true), size = view.getUint32(at + 24, true);
    const nameLength = view.getUint16(at + 28, true), extra = view.getUint16(at + 30, true), comment = view.getUint16(at + 32, true);
    const local = view.getUint32(at + 42, true);
    const name = new TextDecoder("utf-8", { fatal: true }).decode(bytes.subarray(at + 46, at + 46 + nameLength));
    at += 46 + nameLength + extra + comment;
    if (name.endsWith("/")) continue;
    if (flags & 1) throw new Error(`Project archive entry ${name} is encrypted.`);
    if (method !== 0 && method !== 8) throw new Error(`Project archive entry ${name} uses an unsupported compression method.`);
    if (entries.has(name)) throw new Error(`Project archive contains ${name} twice.`);
    limit(name, size);
    if (local + 30 > bytes.length || view.getUint32(local, true) !== LOCAL) throw new Error(`Corrupt project archive entry ${name}.`);
    const start = local + 30 + view.getUint16(local + 26, true) + view.getUint16(local + 28, true);
    if (start + compressed > bytes.length) throw new Error(`Truncated project archive entry ${name}.`);
    const data = bytes.subarray(start, start + compressed);
    const content = method === 0 ? data : await inflate(data, size);
    if (content.length !== size || crc32(content) !== crc) throw new Error(`Project archive entry ${name} is damaged (checksum mismatch).`);
    entries.set(name, content);
  }
  return entries;
}
