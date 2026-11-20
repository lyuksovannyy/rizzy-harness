// Whether a call's choice is up in the open chat (row W.6). Module level, not
// Shell state: Shell's shortcut check reads it, ApprovalChoice raises it while
// it is mounted in the composer's place. Kept tiny: it loads with the shell.
let up = 0;

/** True while a choice is shown in the composer's place. */
export const choiceUp = () => up > 0;

/** The choice is up until the returned function is called (once). */
export function raise(): () => void {
  up++;
  let done = false;
  return () => {
    if (!done) up--;
    done = true;
  };
}
