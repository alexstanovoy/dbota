// A response's bytes, read as they arrive and given up past `limit`: a served file is no larger
// than its bound only if its server is honest. `name` is what the errors call it.
export async function responseBytes(response, limit, name) {
  const length = response.headers.get("content-length");
  if (length !== null && Number(length) > limit) {
    await response.body?.cancel();
    throw new Error(`${name} is ${length} bytes, at most ${limit}`);
  }
  if (!response.body) throw new Error(`${name} has no response body`);
  const reader = response.body.getReader();
  const bytes = new Uint8Array(limit);
  let used = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) return bytes.subarray(0, used);
      if (value.byteLength > limit - used) throw new Error(`${name} exceeds ${limit} bytes`);
      bytes.set(value, used);
      used += value.byteLength;
    }
  } finally {
    await reader.cancel().catch(() => {});
    reader.releaseLock();
  }
}
