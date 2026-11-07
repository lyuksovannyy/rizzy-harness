// Sorting and CSV for GFM tables (pure; Table.tsx renders).

/** A cell as a number: optional sign and currency, thousands commas, a
 * trailing %. Anything else is not a number. */
export function parseNum(cell: string): number | null {
  const s = cell
    .trim()
    .replace(/^[−–]/, "-")
    .replace(/^([-+]?)[$€£¥]/, "$1")
    .replace(/%$/, "");
  if (!/^[-+]?(\d{1,3}(,\d{3})+|\d+)?(\.\d+)?$/.test(s) || !/\d/.test(s)) return null;
  const n = Number(s.replace(/,/g, ""));
  return Number.isFinite(n) ? n : null;
}

/**
 * Row order for sorting by column `col`. Numeric when every non-empty cell
 * in the column is a number, otherwise by locale; empty cells go last either
 * way, and ties keep their original order.
 */
export function sortOrder(rows: string[][], col: number, dir: 1 | -1): number[] {
  const cells = rows.map((r) => (r[col] ?? "").trim());
  const numbers = cells.map((c) => (c === "" ? null : parseNum(c)));
  const numeric = cells.every((c, i) => c === "" || numbers[i] !== null);
  return rows
    .map((_, i) => i)
    .sort((a, b) => {
      const ea = cells[a] === "";
      const eb = cells[b] === "";
      if (ea || eb) return ea === eb ? a - b : ea ? 1 : -1;
      const d = numeric ? numbers[a]! - numbers[b]! : cells[a]!.localeCompare(cells[b]!, undefined, { numeric: true });
      return d === 0 ? a - b : d * dir;
    });
}

/** RFC 4180 CSV. Cells that a spreadsheet would run as a formula get a
 * leading apostrophe: the table text is model output. */
export function toCsv(rows: string[][]): string {
  return rows.map((r) => r.map(csvCell).join(",")).join("\r\n");
}

function csvCell(cell: string): string {
  let c = cell;
  if (/^[=+\-@\t\r]/.test(c) && parseNum(c) === null) c = `'${c}`;
  return /[",\r\n]/.test(c) ? `"${c.replace(/"/g, '""')}"` : c;
}
