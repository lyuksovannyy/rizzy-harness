// FNV-1a, 32 bits, over UTF-8 bytes, as 8 lowercase hex digits. It pairs a
// ui fence with the block Rust stored for it (crates/core/src/ui/scan.rs);
// it is not a security hash, and a collision could only show a block the
// same message validated anyway.
const utf8 = new TextEncoder();

export function fnv1a(text: string): string {
  let h = 0x811c9dc5;
  for (const b of utf8.encode(text)) h = Math.imul(h ^ b, 0x01000193) >>> 0;
  return h.toString(16).padStart(8, "0");
}
