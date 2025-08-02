# Feature Overview

All pages, except `/setup` and pages for login, should check if the user is logined. You can use `v1/user/me` to check the login state, and save the user info in the app state. If the user isn't logined, redirect to `/login`.

## Global Login State Management (Frontend)

*   The frontend global state (`web/src/service/state.js`) now tracks `isLoggedIn`, `user` (containing `username`, `email`, `id`), and `csrfToken`.
*   A global navigation guard (`web/src/router/index.js`) handles:
    *   Initial system initialization check (`/v1/initialized`).
    *   Initial user login status check (`/v1/user/me`).
    *   Redirecting unauthenticated users to `/login`, preserving their intended destination via the `ru` query parameter.
    *   Ensuring `/login` and `/login/telegram` are always accessible.
    *   Redirecting users from `/setup` to `/home` if the system is initialized and they are logged in.

## CSRF Protection (Frontend)

*   The `csrf_token` returned by successful login APIs is now stored in `state.csrfToken`.
*   An `axios` request interceptor (`web/src/service/api.js`) automatically attaches this `csrfToken` to all outgoing API requests as the `x-ne-csrf-token` header.

## Login inside a Browser

In this page, a user can enter his email. Then, the backend server will return a token. The user can send the token within `/login <token>` command in telegram to confirm the login. In the meanwhile, this page will repeatedly check if the login request has been confirmed.

### Points

*   **Path of the web page:** `login`
*   **Path of the auth api:** `v1/user/session` (`POST` for creating a login request, `GET` for getting the login state). `DELETE` for logout (already exists).
*   **Flow:**
    1.  User enters email on `/login` page and submits.
    2.  Frontend (`Login.vue`) sends `POST /v1/user/session` with email.
    3.  Backend returns `token`.
    4.  Frontend displays instructions to the user to send `/login <token>` in Telegram, providing a "Copy Command" button.
    5.  Frontend continuously polls `GET /v1/user/session?token=<token>`.
    6.  When backend confirms login (returns `csrf_token`), frontend saves `state.csrfToken`, sets `state.isLoggedIn = true`, and redirects to `ru` (or `/home` as fallback).

## Login inside a Telegram Mini App

There will be a page which will retrieve `initData` from telegram sdk and `eid` from the query string. Then it will send these data to backend and the backend will verify these data and set a session id to the cooike. Once the verification passed, the page will redirect to the url `ru` from the query string if it exists, otherwise, it will redirect to the home page.

### Points

*   **Path of the web page:** `login/telegram`
*   **Path of the auth api:** `v1/user/session/telegram`
*   **Verification:**
    *   `initData` hash is verified using the bot token.
    *   `initData` is valid only if created in the past 5 minutes.
    *   `eid` (endpoint ID) is checked against the user ID in `initData`.
    *   Most of this verification logic is encapsulated in `auth::verify_telegram_login` (backend).
*   **Flow:**
    1.  Frontend (`LoginTelegram.vue`) dynamically loads Telegram Web App SDK.
    2.  Extracts `initData` from SDK and `eid`, `ru` from URL query.
    3.  Sends `POST /v1/user/session/telegram` with `initData` and `eid`.
    4.  On success, backend creates session, sets session cookie, and returns `csrf_token`.
    5.  Frontend saves `state.csrfToken`, sets `state.isLoggedIn = true`, and redirects to `ru` (or `/home` as fallback).