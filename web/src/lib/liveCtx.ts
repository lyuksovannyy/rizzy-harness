// The page's one live connection, for components deep in a chat that follow
// another session (a sub-agent's card). Null outside a chat.
import { createContext } from "react";
import type { Live } from "../live";

export const LiveCtx = createContext<Live | null>(null);
