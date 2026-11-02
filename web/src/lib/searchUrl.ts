export function buildSearchUrl(template: string, query: string): string {
  return template.replaceAll("{q}", encodeURIComponent(query.trim()));
}
