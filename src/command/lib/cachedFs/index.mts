import { readFileSync } from "node:fs";
import { readFile as readFileAsync } from "node:fs/promises";
import * as path from "node:path";

type Encoding = BufferEncoding;

interface CacheEntry {
  buffer: Buffer;
  texts?: Map<Encoding, string>; // encoding毎の文字列化キャッシュ
  jsonParsed?: unknown; // JSON.parse済みキャッシュ
}

class CachedFsImpl {
  private cache = new Map<string, CacheEntry>();
  private inflight = new Map<string, Promise<Buffer>>(); // 同時多発読みの集約

  /** 絶対パスに正規化してキー化（realpathはコスト高なのでresolveで十分） */
  private key(p: string): string {
    return path.resolve(p);
  }

  /** すべてのキャッシュを即時クリア */
  clear(): void {
    this.inflight.clear();
    this.cache.clear();
  }

  /** キャッシュ有無の確認（任意） */
  has(filePath: string): boolean {
    return this.cache.has(this.key(filePath));
  }

  /** Buffer 読み（同期） */
  readBuffer(filePath: string): Buffer {
    const k = this.key(filePath);
    let entry = this.cache.get(k);
    if (entry) return entry.buffer;

    const buf = readFileSync(k);
    entry = { buffer: buf };
    this.cache.set(k, entry);
    return buf;
  }

  /** Buffer 読み（非同期） */
  async readBufferAsync(filePath: string): Promise<Buffer> {
    const k = this.key(filePath);
    const cached = this.cache.get(k);
    if (cached) return cached.buffer;

    const pending = this.inflight.get(k);
    if (pending) return pending;

    const p = readFileAsync(k).then(
      (buf) => {
        this.cache.set(k, { buffer: buf });
        this.inflight.delete(k);
        return buf;
      },
      (err) => {
        this.inflight.delete(k);
        throw err;
      },
    );

    this.inflight.set(k, p);
    return p;
  }

  /** テキスト読み（同期） */
  readText(filePath: string, encoding: Encoding = "utf8"): string {
    const entry = this.ensureEntry(filePath);
    return this.getText(entry, encoding);
  }

  /** テキスト読み（非同期） */
  async readTextAsync(
    filePath: string,
    encoding: Encoding = "utf8",
  ): Promise<string> {
    const entry = await this.ensureEntryAsync(filePath);
    return this.getText(entry, encoding);
  }

  /** JSON 読み（同期） */
  readJson<T = unknown>(filePath: string): T {
    const entry = this.ensureEntry(filePath);
    return this.getJson<T>(entry);
  }

  /** JSON 読み（非同期） */
  async readJsonAsync<T = unknown>(filePath: string): Promise<T> {
    const entry = await this.ensureEntryAsync(filePath);
    return this.getJson<T>(entry);
  }

  /** 事前ロード（同期） */
  preload(paths: string[]): void {
    for (const p of paths) this.readBuffer(p);
  }

  /** 事前ロード（非同期） */
  async preloadAsync(paths: string[]): Promise<void> {
    await Promise.all(paths.map((p) => this.readBufferAsync(p)));
  }

  // ---- 内部ユーティリティ ----
  private ensureEntry(filePath: string): CacheEntry {
    const k = this.key(filePath);
    let entry = this.cache.get(k);
    if (entry) return entry;

    const buf = readFileSync(k);
    entry = { buffer: buf };
    this.cache.set(k, entry);
    return entry;
  }

  private async ensureEntryAsync(filePath: string): Promise<CacheEntry> {
    const k = this.key(filePath);
    const entry = this.cache.get(k);
    if (entry) return entry;

    await this.readBufferAsync(filePath); // inflight集約経由でロード
    const ret = this.cache.get(k);
    if (!ret) throw new Error("unreachable");
    return ret;
  }

  private getText(entry: CacheEntry, encoding: Encoding): string {
    if (!entry.texts) entry.texts = new Map<Encoding, string>();
    const hit = entry.texts.get(encoding);
    if (hit !== undefined) return hit;

    const s = entry.buffer.toString(encoding);
    entry.texts.set(encoding, s);
    return s;
  }

  private getJson<T>(entry: CacheEntry): T {
    if (entry.jsonParsed !== undefined) return entry.jsonParsed as T;

    // JSONはUTF-8想定。BOMは除去。
    let s = this.getText(entry, "utf8");
    if (s.charCodeAt(0) === 0xfeff) s = s.slice(1);

    const parsed = JSON.parse(s) as T;
    entry.jsonParsed = parsed;
    return parsed;
  }
}

/** どこから import しても同一インスタンス（シングルトン） */
export const CachedFs = new CachedFsImpl();
