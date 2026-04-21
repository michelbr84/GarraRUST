# Garra Mobile Alpha — Manual QA Checklist

> Executable by a human QA engineer before each APK release (Android) / TestFlight cut (iOS). Formalizes **GAR-368**. Keep this file as the canonical release gate — the automated test suite (`tests/e2e_mobile_api.sh`, widget tests, integration tests) covers the golden path, this checklist covers real-device UX and edge cases.

**Reference API contract:** [`docs/mobile-api-v1.yaml`](./mobile-api-v1.yaml).
**App directory:** [`apps/garraia-mobile/`](../apps/garraia-mobile/).
**Backend:** [`crates/garraia-gateway/src/mobile_auth.rs`](../crates/garraia-gateway/src/mobile_auth.rs) + [`mobile_chat.rs`](../crates/garraia-gateway/src/mobile_chat.rs).

## How to use

1. Flash a release build on a clean device (no existing app install).
2. Run each section in order — many items depend on state from earlier items.
3. Mark `[x]` when passing; when failing, open a blocker issue in Linear (team `GAR`) and reference the checklist line.
4. A release is **go** when every item in sections 1–5 passes. Sections 6–7 are non-blocking for Alpha but block Beta.

---

## 1. Installation & cold start

- [ ] Fresh APK install succeeds on Android 11+ (API 30+) without permission prompts beyond network.
- [ ] Fresh IPA install succeeds on iOS 16+ via TestFlight.
- [ ] App icon appears in launcher with the correct Garra branding.
- [ ] App launch cold-start time < 3s on a mid-tier device (Pixel 6a / iPhone 12).
- [ ] Splash screen renders without visual glitches (splash → auth-gated navigation).

## 2. Authentication (GAR-335/336/338)

### Registration

- [ ] `POST /auth/register` with a new email + password ≥8 chars returns 201 and persists token in `flutter_secure_storage` (key `garraia_jwt`).
- [ ] Registration with an already-used email returns 409 and shows a user-visible error message (not a stack trace).
- [ ] Registration with password < 8 chars returns 400 and shows a friendly validation message.
- [ ] Registration with empty email or email without `@` returns 400.
- [ ] After successful registration the user is routed to `/chat` (not back to login).

### Login

- [ ] `POST /auth/login` with valid credentials returns 200 and updates the stored token.
- [ ] Login with wrong password returns 401 with message `invalid credentials` (the same message as unknown email — anti-enumeration).
- [ ] Login with unknown email returns 401 with the same `invalid credentials` message.
- [ ] After successful login the user lands on `/chat` with history hydrated.

### Session persistence

- [ ] Kill + relaunch the app after login — splash screen reads the persisted token and routes directly to `/chat` (no second login prompt).
- [ ] Delete the token via device Settings → App Info → Storage → Clear Data (Android) — next launch goes back to login.
- [ ] `GET /me` returns the same user_id that the stored JWT's `sub` claim decodes to.

### Logout

- [ ] Explicit logout (Settings screen → Logout button) clears the token and routes to `/login`.
- [ ] After logout, relaunching the app shows the login screen (not the last-seen chat).

## 3. Chat (GAR-339/340)

- [ ] Send a message — assistant reply arrives within 15s on an averaged network; the `MascotState` transitions idle → thinking → talking → happy → idle during the round-trip.
- [ ] `GET /chat/history` on `/chat` screen enter populates the message list in correct chronological order.
- [ ] Sent messages appear immediately in the list (optimistic render) and are not duplicated after the server replies.
- [ ] Empty-state (no messages yet) shows a friendly placeholder, not an empty scrollable area.
- [ ] Long message (1000+ characters) sends without truncation and renders with text wrapping in the bubble.
- [ ] Unicode message (emoji, RTL Arabic, Chinese) renders correctly both in sent and received bubbles.
- [ ] `POST /chat` with empty body returns 400 and shows a friendly message (not a 500 error dialog).
- [ ] The composer field clears after a successful send.
- [ ] The mascot `talking` animation plays while new assistant text is streaming in (not just on completion).

## 4. Mascot animations (GAR-357/361/363/364)

The mascot has 4 states: `idle` (breathing), `thinking` (spin), `talking` (bounce), `happy` (jump).

- [ ] On `/chat` with no activity, mascot plays `idle` (subtle breathing).
- [ ] When a message is pending (send → first response byte), mascot switches to `thinking` (continuous spin).
- [ ] When assistant reply is arriving, mascot switches to `talking` (vertical bounce).
- [ ] On a successful round-trip completion, mascot briefly plays `happy` (jump overshoot) then returns to `idle`.
- [ ] State transitions are smooth — no visible jank, no flicker between states.
- [ ] Rive asset fallback (when `assets/garra_mascot.riv` is missing, see GAR-360) — the Flutter animation fallback in `mascot_widget.dart` renders correctly without errors.
- [ ] Mascot size scales correctly on both phone (~96px) and tablet (~128px) form factors.

## 5. Offline queue (GAR-359)

- [ ] With airplane mode ON, sending a message enqueues it locally and shows `SnackBar`: "Mensagem salva para envio quando online".
- [ ] The `QueueStatusIndicator` widget shows the pending count (e.g. "3 mensagens na fila").
- [ ] With airplane mode OFF (return online), messages flush automatically on `AppLifecycleState.resumed` (bring app to foreground).
- [ ] Messages sent via the offline queue appear in the correct order when flushed (FIFO).
- [ ] Kill the app while messages are in the queue, then relaunch — queue is persisted locally and still intact.
- [ ] Backend receives each queued message exactly once (no duplicates on flush retry).

## 6. Error handling

- [ ] Server 500 (simulate by killing `garraia-gateway` process) — app shows a `SnackBar` or toast, does not crash.
- [ ] DNS failure (disconnect WiFi + mobile data) — app shows an offline indicator within 10s, does not hang.
- [ ] Token expired (wait 30 days OR manually inject an expired JWT) — app routes to login gracefully on the next authenticated call.
- [ ] Token tampered (inject a corrupted JWT in secure_storage) — app routes to login, does not crash.
- [ ] HTTP 503 `{"error":"database unavailable"}` — app shows a distinct "servidor em manutenção" message, not a generic failure.

## 7. Performance & polish

- [ ] Cold start → first message sent < 10s on a mid-tier device.
- [ ] Scroll performance in a 500-message history → no frame drops on Pixel 6a (60+ fps in profile mode).
- [ ] Memory footprint after 30 minutes of use < 250 MB on Android.
- [ ] No visible janks during state transitions (page swipes, auth redirect).
- [ ] Dark mode: text legible in all screens; no white flash between navigations.
- [ ] Accessibility: basic VoiceOver / TalkBack reads login fields, chat bubbles, and composer button with meaningful labels.

## 8. Security

- [ ] `adb logcat` (Android) and `idevicesyslog` (iOS) show **no** JWT, no email, no password, no bearer token in any log line during any of the flows above.
- [ ] `flutter_secure_storage` is actually backed by Keystore (Android) / Keychain (iOS) — verify by inspecting app data dump (root/jailbroken device needed).
- [ ] Prod build uses HTTPS base URL (`https://api.garraia.org`) — confirm by intercepting with a proxy and verifying TLS handshake.
- [ ] `localhost:3888` and `10.0.2.2:3888` endpoints are NOT reachable from the prod build (dart-define should have rewritten the base URL).
- [ ] Account-enumeration check: login attempt response timing and body are byte-identical for unknown-email vs. wrong-password. Measure with 100 samples; t-test at p<0.05 should NOT reject the null.

---

## Release decision matrix

| Section | Alpha | Beta | GA |
|---|---|---|---|
| 1. Install & cold start | **BLOCK** | BLOCK | BLOCK |
| 2. Authentication | **BLOCK** | BLOCK | BLOCK |
| 3. Chat | **BLOCK** | BLOCK | BLOCK |
| 4. Mascot | Advisory | BLOCK | BLOCK |
| 5. Offline queue | Advisory | **BLOCK** | BLOCK |
| 6. Error handling | Advisory | **BLOCK** | BLOCK |
| 7. Performance & polish | Advisory | Advisory | **BLOCK** |
| 8. Security | **BLOCK** | BLOCK | BLOCK |

---

## Sign-off

Tester: ____________________
Device: ____________________
Build: ____________________
Date (Florida local): ____________________

Result: **PASS** / **FAIL** (circle one)

If FAIL — blocker issue(s):
- [ ] GAR-____ (describe)
- [ ] GAR-____ (describe)
