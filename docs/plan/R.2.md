# R.2 **sec**

## Findings

P-1 (High), P-2, P-3, P-7

## The change

**A login is bound to its origin, not only to its host.**
A random value kept per origin by the page is required on every cookie-authenticated request and as the first WebSocket message, so another port on the same host cannot use the cookie. `delegate` goes through the approval path and `denied_tools`; the revocation sweep runs again on reconnect; a deny or cancel is never refused.

## Done when

A request with the cookie but without the origin value is refused; the UI works; `denied_tools = ["delegate"]` is honoured.

