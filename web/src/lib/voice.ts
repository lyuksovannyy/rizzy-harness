// The microphone: recorded, then transcribed by the server. Its own chunk,
// loaded at the first click on the mic (out of the first load).
import { api } from "../api";

/** Record from the microphone until the recorder is stopped; then
 * `transcribing`, and `done` with the words or why there are none. */
export async function record(on: { transcribing: () => void; done: (said: string | null, error?: string) => void }): Promise<MediaRecorder> {
  const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
  const r = new MediaRecorder(stream);
  const chunks: Blob[] = [];
  r.ondataavailable = (e) => chunks.push(e.data);
  r.onstop = async () => {
    stream.getTracks().forEach((t) => t.stop());
    on.transcribing();
    // `done` once, outside the try: a throw in it is not a failed transcription.
    let said: string | null = null;
    let error: string | undefined;
    try {
      said = await api.transcribe(new Blob(chunks, { type: r.mimeType || "audio/webm" }));
    } catch (x) {
      error = x instanceof Error ? x.message : String(x);
    }
    on.done(said, error);
  };
  r.start();
  return r;
}
