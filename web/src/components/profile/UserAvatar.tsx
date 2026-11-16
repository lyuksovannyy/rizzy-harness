// A person's avatar where their initial shows: their picture when they have
// one (`version`), loaded through a link (lib/links: an <img> cannot send the
// login key), else the initial. Only lazy chunks use it, so the first load
// (the sidebar's account button) keeps the initial.
import { LinkedImg } from "../Linked";

/** The URL of `userId`'s avatar at `version`: a new picture is a new URL. */
export const avatarUrl = (userId: string, version: string) =>
  `/api/users/${encodeURIComponent(userId)}/avatar?v=${encodeURIComponent(version)}`;

export function UserAvatar(props: { userId: string | null; version: string | null | undefined; name: string; className?: string }) {
  const cls = `avatar ${props.className ?? ""}`;
  return (
    <span className={cls} aria-hidden="true">
      {props.userId && props.version ? (
        <LinkedImg url={avatarUrl(props.userId, props.version)} alt="" className="avatar-img" />
      ) : (
        props.name.slice(0, 1).toUpperCase()
      )}
    </span>
  );
}
