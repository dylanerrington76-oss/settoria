// Sending a map file from the host's computer to the server (`POST /api/maps`, see "Protocol" in the README).
//
// The server does all the checking (it is the same check as the `validate` command) and answers every failure
// with `{ "issues": [..] }`; this file only reads the file, sends it and turns whatever comes back into one
// shape the lobby can show. No React in here, so the tests can run it in Node.

/** The biggest map file the server accepts (`MAX_MAP_BYTES` in src/map_def.rs). Checked here too, to save the trip. */
export const MAX_MAP_BYTES = 256 * 1024;

/** The most files taken from one pick or drop. The server keeps at most 10 uploaded maps (`MAX_UPLOADS` in src/maps.rs). */
export const MAX_FILES_AT_ONCE = 10;

export type UploadResult =
  | { ok: true; file: string; id: string; name: string; warnings: string[] }
  | { ok: false; file: string; issues: string[] };

/** The part of `fetch` this uses, so a test can stand in for the network. */
export type Post = (url: string, init: { method: 'POST'; headers: Record<string, string>; body: string }) => Promise<{
  status: number;
  text(): Promise<string>;
}>;

const strings = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : []);

/**
 * Sends one map file. `token` is the host's seat token (the one from `welcome`), which is how the server knows the
 * request comes from the host. Never throws: every way it can go wrong is an `ok: false` with sentences to show.
 */
export async function uploadMapFile(
  file: { name: string; size: number; text(): Promise<string> },
  token: string | null,
  post: Post = (url, init) => fetch(url, init),
): Promise<UploadResult> {
  const fail = (...issues: string[]): UploadResult => ({ ok: false, file: file.name, issues });
  if (!token) return fail('You need a seat at the table before you can upload a map.');
  if (file.size === 0) return fail('The file is empty.');
  if (file.size > MAX_MAP_BYTES) return fail(`The file is too big: ${Math.ceil(file.size / 1024)} KB, and the limit is ${MAX_MAP_BYTES / 1024} KB.`);

  let body: string;
  try {
    body = await file.text();
  } catch {
    return fail('The file could not be read.');
  }

  let res;
  try {
    res = await post('/api/maps', { method: 'POST', headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` }, body });
  } catch {
    return fail('Could not reach the server.');
  }

  let json: unknown = null;
  try {
    json = JSON.parse(await res.text());
  } catch {
    // not JSON (a proxy's error page, say): the status below is all there is to go on
  }
  const obj = json && typeof json === 'object' ? (json as Record<string, unknown>) : {};
  if (res.status === 201 && typeof obj.id === 'string') {
    return { ok: true, file: file.name, id: obj.id, name: typeof obj.name === 'string' ? obj.name : obj.id, warnings: strings(obj.warnings) };
  }
  const issues = strings(obj.issues);
  return fail(...(issues.length > 0 ? issues : [`The server answered ${res.status} and the map was not added.`]));
}
