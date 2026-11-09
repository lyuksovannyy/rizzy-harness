// FNV-1a, 32 bits, over the string's UTF-16 code units, as an unsigned
// number. For seeds and bucket indices only (avatars, the wall's random
// streams, the classifier's feature hashing); not a security hash.
//
// md/fnv.ts is deliberately a different function: it hashes UTF-8 bytes and
// returns hex, to match crates/core/src/ui/scan.rs. The two agree only on
// ASCII, so they must not be merged.
export function fnv1a(s: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}
