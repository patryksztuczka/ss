// Electron main process for the compose reference: one hidden window, no Dock
// icon, user data in a temp folder. Input and output paths come from env.
const { app, BrowserWindow } = require("electron");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

app.setPath("userData", fs.mkdtempSync(path.join(os.tmpdir(), "ss-compose-ref-")));
app.dock?.hide();

app.whenReady().then(async () => {
  const window = new BrowserWindow({ show: false, width: 400, height: 300 });
  await window.loadURL("data:text/html,<!doctype html><title>compose</title>");
  await window.webContents.executeJavaScript(fs.readFileSync(process.env.SS_BUNDLE, "utf8"));
  const source = fs.readFileSync(process.env.SS_SOURCE).toString("base64");
  const styles = JSON.parse(process.env.SS_STYLES);
  const result = await window.webContents.executeJavaScript(
    `renderReference(${JSON.stringify(source)}, ${JSON.stringify(styles)})`,
  );
  const out = process.env.SS_OUT;
  fs.writeFileSync(path.join(out, "source-srgb.png"), Buffer.from(result.source, "base64"));
  const manifest = { electron: process.versions.electron, chrome: process.versions.chrome, insetColor: result.insetColor, cases: [] };
  result.outputs.forEach((output, index) => {
    const file = `case-${index}.png`;
    fs.writeFileSync(path.join(out, file), Buffer.from(output.png, "base64"));
    manifest.cases.push({ file, style: output.style, layout: output.layout });
  });
  fs.writeFileSync(path.join(out, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
  app.quit();
});
