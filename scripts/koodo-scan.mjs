// Koodo Reader 真机抽验扫描：重启 Koodo → 命令行开书 → PageDown 逐页翻全书，
// 收集每章标题/文本摘要/pre 计数，截图存 tmp/koodo/。配套 scripts/cdp-driver.mjs。
// 用法：node scripts/koodo-scan.mjs <epub绝对路径> <截图前缀> [翻页次数=18]
// 依赖：Koodo Reader（scoop koodo-reader；路径见 EXE 常量，按本机安装位置调整）。
// 2026-10-26 首轮真机抽验（flat/tree/mdbook/web/single/嵌字体 6 本）验证通过。
import { spawn, execSync } from "node:child_process";
import { httpGetJson, connect } from "./cdp-driver.mjs";

const PORT = 9223;
const EXE = "D:/Software/Scoop/apps/koodo-reader/current/Koodo Reader.exe";
const epub = process.argv[2];
const prefix = process.argv[3] || "scan";
const maxPages = Number(process.argv[4] || 18);

if (!epub) { console.log("usage: node koodo-scan.mjs <epub> <prefix> [pages]"); process.exit(1); }

// 全量重启：清掉旧 reader target，保证连接到的是本本书
try { execSync('taskkill /IM "Koodo Reader.exe" /F', { stdio: "ignore" }); } catch {}
await new Promise((r) => setTimeout(r, 2000));
spawn(EXE, ["--remote-debugging-port=" + PORT], { detached: true, stdio: "ignore" }).unref();
await new Promise((r) => setTimeout(r, 3000));
spawn(EXE, [epub], { detached: true, stdio: "ignore" }).unref();
await new Promise((r) => setTimeout(r, 5000));

const list = await httpGetJson(`http://127.0.0.1:${PORT}/json/list`);
const reader = list.filter((t) => t.type === "page" && t.url.includes("#/epub/")).pop();
if (!reader) { console.log("no reader target:", JSON.stringify(list.map((t) => t.title))); process.exit(1); }
const ws = await connect(reader.webSocketDebuggerUrl);
await ws.cmd("Runtime.enable");
await ws.cmd("Page.enable");

async function pressPageDown() {
  for (const type of ["keyDown", "keyUp"]) {
    await ws.cmd("Input.dispatchKeyEvent", {
      type, key: "PageDown", code: "PageDown",
      windowsVirtualKeyCode: 34, nativeVirtualKeyCode: 34,
    });
  }
}

const snap = () => ws.evaluate(`(() => {
  const f = document.querySelector("iframe");
  if (!f) return null;
  const doc = f.contentDocument;
  const heads = [...doc.querySelectorAll("h1,h2,h3")].slice(0, 4).map((h) => h.textContent.trim().slice(0, 50));
  const text = (doc.body.innerText || "").replace(/\\s+/g, " ").slice(0, 110);
  return { heads, text, preCount: doc.querySelectorAll("pre").length };
})()`);

const chapters = [];
let lastKey = "";
let stagnant = 0;
for (let i = 0; i < maxPages; i++) {
  const s = await snap();
  if (!s) break;
  const key = s.heads.join("|") + "#" + s.text.slice(0, 40);
  if (key !== lastKey) {
    chapters.push(s);
    lastKey = key;
    stagnant = 0;
  } else {
    stagnant++;
    if (stagnant >= 3) break; // 连续 3 次无变化视为到书末或卡住
  }
  if (i === 2) await ws.screenshot(`tmp/koodo/${prefix}-early.png`);
  await pressPageDown();
  await new Promise((r) => setTimeout(r, 850));
}
await ws.screenshot(`tmp/koodo/${prefix}-last.png`);
console.log("== chapters seen ==");
for (const c of chapters) {
  console.log(`- H:${c.heads.join(" / ") || "(none)"} pre=${c.preCount}`);
  console.log(`  T:${c.text}`);
}
ws.close();
