/** The server's floor (auth::validate_password). */
export const MIN_PASSWORD = 10;

export type Strength = { score: 0 | 1 | 2 | 3 | 4; label: string };

const LABELS = ["Too short", "Weak", "Fair", "Good", "Strong"] as const;

/** A rough client-side guide, not a guarantee: length counts most, then
 * mixing kinds of characters; repeats and runs like "aaaa" or "1234" cost. */
export function passwordStrength(pw: string): Strength {
  const len = [...pw].length;
  if (len < MIN_PASSWORD) return { score: 0, label: LABELS[0] };
  const kinds = [/[a-z]/, /[A-Z]/, /[0-9]/, /[^a-zA-Z0-9]/].filter((r) => r.test(pw)).length;
  let points = (len >= 14 ? 1 : 0) + (len >= 18 ? 1 : 0) + (len >= 24 ? 1 : 0) + (kinds - 1);
  if (new Set(pw.toLowerCase()).size < len / 2) points -= 1;
  if (/(.)\1{2,}|0123|1234|2345|3456|4567|5678|6789|abcd|qwer|asdf/i.test(pw)) points -= 1;
  const score = Math.max(1, Math.min(4, 1 + Math.floor(points / 1.5))) as Strength["score"];
  return { score, label: LABELS[score] };
}
