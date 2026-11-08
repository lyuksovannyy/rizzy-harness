// File sizes as people read them. Its own module because Media, Artifacts,
// the Storage tab and the chat's artifact cards all say sizes, and the chat
// should not load the Media page's code to do it.

/** 1536 → "1.5 KB": steps of 1024, as the quota counts. */
export function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u++;
  }
  return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${units[u]}`;
}
