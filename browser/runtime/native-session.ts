/** A project-scoped revision stream on the loopback native host. */
export class NativeSession {
  private session?: Promise<string>;
  private serial = 0;
  private active?: AbortController;
  private readonly headers = { "X-SuperSpec": "1" };

  private open(): Promise<string> {
    if (this.session) return this.session;
    const opening = fetch("/api/session", { method: "POST", headers: this.headers }).then(async response => {
      if (!response.ok) throw Error(`Could not open native Studio (${response.status}).`);
      return (await response.json()).session as string;
    });
    this.session = opening;
    void opening.catch(() => { if (this.session === opening) this.session = undefined; });
    return opening;
  }
  async request(action: string, body: Blob, input = "project"): Promise<Response> {
    this.active?.abort();
    const controller = this.active = new AbortController();
    const release = () => { if (this.active === controller) this.active = undefined; };
    let reopened = false;
    try {
      for (let attempt = 0; ; attempt++) {
        const revision = ++this.serial;
        const key = await this.open();
        controller.signal.throwIfAborted();
        const response = await fetch(`/api/session/${key}/${action}`, {
          method: "POST", body, signal: controller.signal,
          headers: { ...this.headers, "X-Revision": String(revision), "X-Input": input },
        });
        if (response.status === 410 && !reopened) {
          await response.body?.cancel();
          controller.signal.throwIfAborted();
          this.session = undefined;
          reopened = true;
          continue;
        }
        if (response.status !== 503 || attempt >= 5) {
          if (!response.body) { release(); return response; }
          // Fetch resolves at headers. Keep the revision cancellable while the client reads the body.
          const body = new TransformStream<Uint8Array, Uint8Array>({ flush: release });
          void response.body.pipeTo(body.writable, { signal: controller.signal }).then(release, release);
          return new Response(body.readable, response);
        }
        await response.body?.cancel();
        await new Promise(resolve => setTimeout(resolve, 25 * (2 ** attempt)));
        controller.signal.throwIfAborted();
      }
    } catch (error) { release(); throw error; }
  }
  cancel() {
    if (!this.active) return;
    this.active.abort();
    this.active = undefined;
    const revision = ++this.serial;
    void this.session?.then(key => fetch(`/api/session/${key}/cancel`, {
      method: "POST", headers: { ...this.headers, "X-Revision": String(revision) },
    })).catch(() => {});
  }
  close() {
    this.active?.abort();
    this.active = undefined;
    void this.session?.then(key => fetch(`/api/session/${key}/close`, {
      method: "POST", headers: this.headers, keepalive: true,
    })).catch(() => {});
    this.session = undefined;
  }
}
