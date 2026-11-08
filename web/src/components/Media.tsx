// Sidebar → Media (docs/media.md): the grid of the account's images, video and audio. A lazy chunk of its own; until the page
// lands it says so, and no sidebar entry leads here.
import type { MediaPageProps } from "./pages";

export default function Media(_props: MediaPageProps) {
  return (
    <section>
      <p className="muted pad">Not available yet.</p>
    </section>
  );
}
