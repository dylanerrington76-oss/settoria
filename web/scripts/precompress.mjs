// Writes a brotli (.br) and a gzip (.gz) copy beside every text file in web/dist, which `npm run build` runs after Vite.
// The server (src/main.rs, `router`) sends whichever one the browser accepts, so nothing is compressed while a
// request waits, and each file is squeezed as hard as it will go.
//
//   node scripts/precompress.mjs [dist-folder]        (the folder is web/dist unless one is given)
//
// It needs nothing but Node: zlib has both encoders. Fonts and images are left alone, since they are compressed
// already. A copy is kept only if it is smaller than the file, and a stale copy of a file that no longer gets one
// is removed, so the server never sends an old body.
import { brotliCompressSync, constants, gzipSync } from 'node:zlib';
import { existsSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, extname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const TEXT = new Set(['.html', '.js', '.mjs', '.css', '.json', '.svg', '.txt', '.xml', '.webmanifest', '.map']);

const dist = resolve(process.argv[2] ?? join(dirname(fileURLToPath(import.meta.url)), '..', 'dist'));
if (!existsSync(dist)) {
  console.error(`precompress: ${dist} does not exist; build the client first`);
  process.exit(1);
}

/** Every file under `dir`, as full paths. */
function* walk(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) yield* walk(path);
    else if (entry.isFile()) yield path;
  }
}

const brotli = (bytes) =>
  brotliCompressSync(bytes, {
    params: {
      [constants.BROTLI_PARAM_QUALITY]: constants.BROTLI_MAX_QUALITY,
      [constants.BROTLI_PARAM_MODE]: constants.BROTLI_MODE_TEXT,
      [constants.BROTLI_PARAM_SIZE_HINT]: bytes.length,
    },
  });
const gzip = (bytes) => gzipSync(bytes, { level: 9 });

/** Write `path` + `ext` if `packed` is smaller than the original, else make sure no such file is left. */
function keep(path, ext, bytes, packed) {
  const out = path + ext;
  if (packed.length < bytes.length) {
    writeFileSync(out, packed);
    return packed.length;
  }
  rmSync(out, { force: true });
  return null;
}

const files = [...walk(dist)];
const sources = files.filter((f) => TEXT.has(extname(f)) && !f.endsWith('.br') && !f.endsWith('.gz'));
// A .br or .gz whose original is gone (renamed, or not text any more) would be served for nothing; drop it.
const sourceSet = new Set(sources);
for (const f of files) {
  if ((f.endsWith('.br') || f.endsWith('.gz')) && !sourceSet.has(f.slice(0, -3))) rmSync(f);
}

let raw = 0;
let br = 0;
let gz = 0;
console.log('file'.padEnd(44), 'raw'.padStart(9), 'gzip'.padStart(9), 'brotli'.padStart(9));
for (const file of sources) {
  const bytes = readFileSync(file);
  const g = keep(file, '.gz', bytes, gzip(bytes));
  const b = keep(file, '.br', bytes, brotli(bytes));
  raw += bytes.length;
  gz += g ?? bytes.length;
  br += b ?? bytes.length;
  console.log(file.slice(dist.length + 1).padEnd(44), String(statSync(file).size).padStart(9), String(g ?? '-').padStart(9), String(b ?? '-').padStart(9));
}
console.log('total'.padEnd(44), String(raw).padStart(9), String(gz).padStart(9), String(br).padStart(9));
