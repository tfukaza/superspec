/**
 * A minimal MP4 writer for one H.264 video track, for pictures the browser encodes with WebCodecs
 * (`VideoEncoder` with `avc: { format: "avc" }`): the encoder's `decoderConfig.description` is the avcC record and
 * each chunk is length-prefixed NAL units. Frames are evenly spaced (`fps`), so timing never depends on how long a
 * frame took to draw. Writes ftyp, a fast-start moov (sample sizes, key frames, offsets) and one mdat.
 */
const u32 = n => { const b = Buffer.alloc(4); b.writeUInt32BE(n >>> 0); return b; };
const u16 = n => { const b = Buffer.alloc(2); b.writeUInt16BE(n); return b; };
const box = (type, ...parts) => { const body = Buffer.concat(parts); return Buffer.concat([u32(body.length + 8), Buffer.from(type, "latin1"), body]); };
const full = (type, version, flags, ...parts) => box(type, Buffer.from([version, (flags >> 16) & 255, (flags >> 8) & 255, flags & 255]), ...parts);
const matrix = Buffer.concat([0x00010000, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000].map(u32));

/**
 * @param {{ width: number, height: number, fps: number, description: Uint8Array,
 *   samples: { data: Uint8Array, key: boolean }[] }} video
 * @returns {Buffer}
 */
export function mp4(video) {
  const { width, height, fps, description, samples } = video;
  const timescale = fps * 1000, delta = 1000, duration = samples.length * delta;
  const sizes = samples.map(s => s.data.length);
  const build = mdatOffset => {
    const avc1 = box("avc1", Buffer.alloc(6), u16(1), Buffer.alloc(16), u16(width), u16(height), u32(0x00480000), u32(0x00480000),
      u32(0), u16(1), Buffer.alloc(32), u16(0x18), u16(0xffff), box("avcC", Buffer.from(description)));
    const stbl = box("stbl",
      full("stsd", 0, 0, u32(1), avc1),
      full("stts", 0, 0, u32(1), u32(samples.length), u32(delta)),
      full("stss", 0, 0, u32(samples.filter(s => s.key).length), ...samples.flatMap((s, i) => s.key ? [u32(i + 1)] : [])),
      full("stsc", 0, 0, u32(1), u32(1), u32(samples.length), u32(1)),
      full("stsz", 0, 0, u32(0), u32(samples.length), ...sizes.map(u32)),
      full("stco", 0, 0, u32(1), u32(mdatOffset + 8)));
    const minf = box("minf", full("vmhd", 0, 1, Buffer.alloc(8)),
      box("dinf", full("dref", 0, 0, u32(1), full("url ", 0, 1))), stbl);
    const mdia = box("mdia",
      full("mdhd", 0, 0, u32(0), u32(0), u32(timescale), u32(duration), u16(0x55c4), u16(0)),
      full("hdlr", 0, 0, u32(0), Buffer.from("vide", "latin1"), Buffer.alloc(12), Buffer.from("VideoHandler\0", "latin1")),
      minf);
    const tkhd = full("tkhd", 0, 3, u32(0), u32(0), u32(1), u32(0), u32(duration), Buffer.alloc(8), u16(0), u16(0), u16(0), u16(0),
      matrix, u32(width << 16), u32(height << 16));
    const mvhd = full("mvhd", 0, 0, u32(0), u32(0), u32(timescale), u32(duration), u32(0x00010000), u16(0x0100), Buffer.alloc(10),
      matrix, Buffer.alloc(24), u32(2));
    return box("moov", mvhd, box("trak", tkhd, mdia));
  };
  const ftyp = box("ftyp", Buffer.from("isom", "latin1"), u32(512), Buffer.from("isomiso2avc1mp41", "latin1"));
  const moovSize = build(0).length;
  const moov = build(ftyp.length + moovSize);
  const payload = Buffer.concat(samples.map(s => Buffer.from(s.data)));
  return Buffer.concat([ftyp, moov, u32(payload.length + 8), Buffer.from("mdat", "latin1"), payload]);
}
