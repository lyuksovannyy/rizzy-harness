// The images a user message carries (docs/media.md §6.6): small pictures
// under the bubble that open the shared MediaPreview (its own lazy chunk).
// The bytes live in Media, so an image deleted there shows "removed from
// Media" here; the event keeps only its name and size.
import { lazy, Suspense, useState } from "react";
import { contentUrl, type MediaItem, mediaApi, safeToDecode } from "../lib/media";
import type { ImageRef } from "../types";

const MediaPreview = lazy(() => import("./MediaPreview"));

export function BubbleImages({ images }: { images: ImageRef[] }) {
  const [gone, setGone] = useState<string[]>([]);
  const [open, setOpen] = useState<MediaItem | null>(null);
  const lose = (id: string) => setGone((g) => [...g, id]);
  return (
    <>
      <ul className="chips bubble-images">
        {images.map((r) => (
          <li key={r.media_id} className={gone.includes(r.media_id) ? "chip muted" : undefined}>
            {gone.includes(r.media_id) ? (
              `${r.name}: removed from Media`
            ) : (
              <button type="button" className="bubble-image" title={r.name} onClick={() => mediaApi.get(r.media_id).then(setOpen, () => lose(r.media_id))}>
                {/* The header's size, as Media checks it: nothing too large is decoded. */}
                {safeToDecode({ kind: "image", display: "inline", format: "", width: r.width, height: r.height }) ? (
                  <img src={contentUrl(r.media_id)} alt={r.name} width={r.width} height={r.height} loading="lazy" onError={() => lose(r.media_id)} />
                ) : (
                  r.name
                )}
              </button>
            )}
          </li>
        ))}
      </ul>
      {open && (
        <Suspense fallback={null}>
          <MediaPreview
            item={open}
            onClose={() => setOpen(null)}
            onChanged={setOpen}
            onDeleted={(id) => {
              setOpen(null);
              lose(id);
            }}
          />
        </Suspense>
      )}
    </>
  );
}
