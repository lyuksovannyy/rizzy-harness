# W.8

## Covers

What the browser run turned up

## Change

(1) `DeleteChat.tsx` `confirm()` swallows a failed delete (`.catch(() => {})`) and goes home as if it worked: show the error and stay (**done** on `feat/web-batch-2`; (2) and (3) were not looked at there). (2) The scripts' `keyed()` helper patches `window.fetch` to add the login key, so a UI request that forgot the header would pass in the nine scripts that use it: the scripts' own requests take the key explicitly instead. (3) A new Agentic chat starts on the Assistant, which has no file or command tools, so nothing can run until the agent is changed, and a routine set up that way just ends: the default agent per mode (W.4) moves up.

## Done when, and size

A component test for the failed delete; `keyed()` gone and the scripts still pass; a new Agentic chat can run a command without touching the picker.
**S** each

