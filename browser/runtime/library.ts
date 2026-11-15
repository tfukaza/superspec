const shipped = import.meta.glob<string>("../../assets/materials/*.{png,jpg,jpeg}", { query: "?url", import: "default", eager: true });
const urls = new Map(Object.entries(shipped).map(([path, url]) => [path.split("/").at(-1)!, url]));
const loaded = new Map<string, Promise<Uint8Array>>();

/** Resolves explicit catalog identities from the engine distribution; authored paths stay host-supplied. */
export function libraryAsset(name: string): Promise<Uint8Array> {
  let pending = loaded.get(name);
  if (!pending) {
    pending = (async () => {
      if (!name.startsWith("superspec-material:")) throw new Error(`Not a catalog identity: ${name}.`);
      const url = urls.get(name.slice("superspec-material:".length));
      if (!url) throw new Error(`Missing shipped material ${name}.`);
      const response = await fetch(url);
      if (!response.ok) throw new Error(`Cannot load ${name} (${response.status}).`);
      return new Uint8Array(await response.arrayBuffer());
    })().catch(error => { loaded.delete(name); throw error; });
    loaded.set(name, pending);
  }
  return pending;
}
