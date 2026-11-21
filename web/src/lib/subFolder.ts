/** Where a sub-agent that can change files worked, in words (the delegate card and the child chat's banner). */
export function folderWords(f: string | undefined | null): string | null {
  return f === "shared" ? "in this chat's folder" : f === "own" ? "in a copy of the folder of its own" : null;
}
