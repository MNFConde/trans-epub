// 最小 CDP 驱动：node:http 拿 target 列表 + 手写 WebSocket 客户端（node:net + node:crypto）。
// kernel 剥掉了全局 fetch/WebSocket，但 node:* 内建齐全，于是自己实现 RFC6455 帧层。
// 只服务 127.0.0.1 上的 DevTools 协议：文本帧、掩码、分片、ping/pong。
// 用途：真机抽验时驱动带 --remote-debugging-port 的 Electron 阅读器（如 Koodo Reader）。
// 2026-10-25 tmp 探针（Readest 抽验），2026-10-26 第 3 次使用（Koodo 抽验）按三次法则固化。
import net from "node:net";
import http from "node:http";
import crypto from "node:crypto";
import fs from "node:fs";

export function httpGetJson(url) {
  const m = /^http:\/\/([^:/]+):(\d+)(\/.*)$/.exec(url);
  if (!m) throw new Error("bad http url: " + url);
  return new Promise((resolve, reject) => {
    const req = http.get({ host: m[1], port: Number(m[2]), path: m[3] }, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => {
        try { resolve(JSON.parse(Buffer.concat(chunks).toString())); }
        catch (e) { reject(new Error("non-json response: " + Buffer.concat(chunks).toString().slice(0, 200))); }
      });
    });
    req.on("error", reject);
    req.setTimeout(5000, () => { req.destroy(new Error("http timeout")); });
  });
}

export async function connect(wsUrl) {
  const m = /^ws:\/\/([^:/]+):(\d+)(\/.*)$/.exec(wsUrl);
  if (!m) throw new Error("bad ws url: " + wsUrl);
  const [, host, portStr, path] = m;
  const socket = net.connect({ host, port: Number(portStr) });
  await new Promise((res, rej) => { socket.once("connect", res); socket.once("error", rej); });

  const key = crypto.randomBytes(16).toString("base64");
  socket.write(
    `GET ${path} HTTP/1.1\r\nHost: ${host}:${portStr}\r\nUpgrade: websocket\r\n` +
    `Connection: Upgrade\r\nSec-WebSocket-Key: ${key}\r\nSec-WebSocket-Version: 13\r\n\r\n`,
  );
  let handshake = Buffer.alloc(0);
  await new Promise((res, rej) => {
    const onData = (chunk) => {
      handshake = Buffer.concat([handshake, chunk]);
      const idx = handshake.indexOf("\r\n\r\n");
      if (idx >= 0) { socket.off("data", onData); socket.unshift(handshake.subarray(idx + 4)); res(); }
    };
    socket.on("data", onData);
    socket.once("error", rej);
    socket.setTimeout(5000, () => socket.destroy(new Error("ws handshake timeout")));
  });
  socket.setTimeout(0);
  const head = handshake.subarray(0, handshake.indexOf("\r\n\r\n")).toString();
  if (!/^HTTP\/1\.1 101/.test(head.split("\r\n")[0])) throw new Error("ws handshake failed: " + head);

  let buf = Buffer.alloc(0);
  const pendingCommands = new Map();
  const listeners = [];
  let seq = 0;

  function frame(opcode, payload) {
    const mask = crypto.randomBytes(4);
    const masked = Buffer.allocUnsafe(payload.length);
    for (let i = 0; i < payload.length; i++) masked[i] = payload[i] ^ mask[i % 4];
    let header;
    if (payload.length < 126) header = Buffer.from([0x80 | opcode, 0x80 | payload.length]);
    else if (payload.length < 65536) {
      header = Buffer.alloc(4); header[0] = 0x80 | opcode; header[1] = 0x80 | 126; header.writeUInt16BE(payload.length, 2);
    } else {
      header = Buffer.alloc(10); header[0] = 0x80 | opcode; header[1] = 0x80 | 127; header.writeBigUInt64BE(BigInt(payload.length), 2);
    }
    return Buffer.concat([header, mask, masked]);
  }

  let msgParts = null; // { opcode, parts: Buffer[] }
  function deliver(text) {
    let obj;
    try { obj = JSON.parse(text); } catch { return; }
    if (obj.id !== undefined && pendingCommands.has(obj.id)) {
      const { resolve } = pendingCommands.get(obj.id);
      pendingCommands.delete(obj.id);
      resolve(obj);
    } else {
      for (const l of listeners) l(obj);
    }
  }
  function handleMessage(fin, opcode, payload) {
    if (opcode === 0x9) { socket.write(frame(0x0a, payload)); return; } // ping -> pong
    if (opcode === 0x8) { socket.end(); return; }                        // close
    if (opcode === 0x1 || opcode === 0x2) msgParts = { parts: [payload] };
    else if (opcode === 0x0 && msgParts) msgParts.parts.push(payload);
    else return;
    if (fin) {
      const text = Buffer.concat(msgParts.parts).toString();
      msgParts = null;
      deliver(text);
    }
  }
  function tryParse() {
    for (;;) {
      if (buf.length < 2) return;
      const fin = (buf[0] & 0x80) !== 0;
      const opcode = buf[0] & 0x0f;
      const masked = (buf[1] & 0x80) !== 0;
      let len = buf[1] & 0x7f;
      let off = 2;
      if (len === 126) { if (buf.length < 4) return; len = buf.readUInt16BE(2); off = 4; }
      else if (len === 127) { if (buf.length < 10) return; len = Number(buf.readBigUInt64BE(2)); off = 10; }
      let maskKey = null;
      if (masked) { if (buf.length < off + 4) return; maskKey = buf.subarray(off, off + 4); off += 4; }
      if (buf.length < off + len) return;
      let payload = buf.subarray(off, off + len);
      if (maskKey) {
        const out = Buffer.allocUnsafe(len);
        for (let i = 0; i < len; i++) out[i] = payload[i] ^ maskKey[i % 4];
        payload = out;
      }
      buf = buf.subarray(off + len);
      handleMessage(fin, opcode, payload);
    }
  }
  socket.on("data", (chunk) => { buf = Buffer.concat([buf, chunk]); tryParse(); });

  const session = {
    socket,
    on(fn) { listeners.push(fn); },
    cmd(method, params = {}) {
      return new Promise((resolve, reject) => {
        const id = ++seq;
        pendingCommands.set(id, { resolve });
        socket.write(frame(0x1, Buffer.from(JSON.stringify({ id, method, params }))));
        setTimeout(() => {
          if (pendingCommands.has(id)) { pendingCommands.delete(id); reject(new Error(`cdp timeout: ${method}`)); }
        }, 20000);
      });
    },
    async evaluate(expression, contextId) {
      const params = { expression, returnByValue: true, awaitPromise: true };
      if (contextId !== undefined) params.contextId = contextId;
      const r = await session.cmd("Runtime.evaluate", params);
      if (r.error) throw new Error("cdp error " + JSON.stringify(r.error));
      const d = r.result && r.result.exceptionDetails;
      if (d) throw new Error("page exception: " + ((d.exception && d.exception.description) || d.text));
      return r.result && r.result.result ? r.result.result.value : undefined;
    },
    async screenshot(savePath) {
      const r = await session.cmd("Page.captureScreenshot", { format: "png" });
      if (r.error) throw new Error("screenshot failed: " + JSON.stringify(r.error));
      const bytes = Buffer.from(r.result.data, "base64");
      if (savePath) fs.writeFileSync(savePath, bytes);
      return bytes;
    },
    close() { socket.end(); },
  };
  return session;
}
