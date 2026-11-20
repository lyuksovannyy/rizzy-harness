# T.5 **sec**

## Adds

API keys per user and per workspace (O5; the owner, 2026-11-12)

## Safeguards and abuse tests, written first

The single static token is removed.
A user's key acts as that user.
A workspace key is its own identity inside one workspace: it owns the chats and routines it creates and reads nobody's private chats; creating one needs a permission of its own, which the workspace role "admin" does not imply by default.
Keys are shown once, stored as a hash, scoped, revocable, with an expiry and a last-used time.
Tests first: A's key cannot read B's chat; a workspace key cannot read any user's chat and reaches no other workspace; a member without the permission cannot create one; a revoked or expired key is 401 at once; no key in a log line, an event or a URL.

## Invariants

Private chats, restored

## Podman / Node

No / Yes

## Depends

fix plan 4.1

## Size

**M to L**

