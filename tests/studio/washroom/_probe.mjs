import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { resolve, extname } from "node:path";
import { chromium } from "playwright";
import { buildProject } from "file:///C:/Users/tomok/OneDrive/code/superspec/tests/studio/washroom/build.mjs";
const root = resolve("C:/Users/tomok/OneDrive/code/superspec/generated/rust-editor");
const server = createServer(async (request, response) => {
  try {
    const name = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
    const path = resolve(root, name === "/" ? "index.html" : `.${name}`);
    response.setHeader("Content-Type", { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".wasm": "application/wasm", ".png": "image/png" }[extname(path)] ?? "application/octet-stream");
    response.end(await readFile(path));
  } catch { response.writeHead(404).end(); }
});
await new Promise(done => server.listen(0, "127.0.0.1", done));
const { project } = buildProject(Number(process.argv[2] ?? 64));
if (process.argv[3]) project.materials = project.materials.slice(0, Number(process.argv[3]));
const browser = await chromium.launch();
const page = await browser.newPage();
page.on("console", m => console.log("console:", m.text().slice(0, 300)));
page.on("pageerror", e => console.log("pageerror:", e.message));
await page.goto(`http://127.0.0.1:${server.address().port}/material.html`);
await page.waitForFunction(() => document.querySelector("#status")?.dataset.state === "ready");
const t = Date.now();
await page.locator("#open-project").setInputFiles({ name: "washroom.studio.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(project)) });
for (let i = 0; i < Number(process.argv[4] ?? 20); i++) {
  await page.waitForTimeout(3000);
  const s = await page.evaluate(() => [document.querySelector("#status").dataset.state, document.querySelector("#status").textContent, document.querySelector("#viewport").dataset.inputRevision, [...document.querySelectorAll("#diagnostics button")].map(b => b.textContent).join("|").slice(0, 300)]);
  console.log(Date.now() - t, JSON.stringify(s));
  if (s[0] === "ready" && i > 0) break;
}
await browser.close(); server.close();
