---
title: Make It Even Faster
description: "Use a personal Spotify app for a separate quota while shared coverage stays active."
nav_order: 4
---

## API rate limits

Oxidify loads library and catalogue data through Spotify's Web API, which
is rate-limited per *app*. By default, Oxidify shares a public app with
several other open-source players. When that app reaches its limit, requests
are delayed and the top bar shows a spinner.

An app of your own gives supported requests a separate Development Mode
quota. Oxidify cannot ship one for everyone, but making yours is free and
takes a few minutes.

## Shared coverage stays active

Spotify keeps a personal app in Development Mode, and since February 2026 that
mode omits Spotify-owned playlists and reads playlist items only for playlists
you own or collaborate on. Artist top tracks, related artists,
recommendations, and some catalog fields are unavailable too. Oxidify uses
the shared app for the complete playlist library, playlist-bearing search,
external playlist metadata and items, and those unavailable operations. Your
app accelerates supported work without replacing shared coverage.

## Make a Spotify app

1. Open the [Spotify developer dashboard](https://developer.spotify.com/dashboard)
   and sign in with your Spotify account. Spotify asks that it be a
   Premium account.
2. Click **Create app**. Any name and description will do; nobody else
   sees them.
3. Under **Redirect URIs**, add exactly:

   ```
   http://127.0.0.1:8989/login
   ```

4. Tick **Web API**, accept the terms, and save.
5. The app's page shows its **Client ID**. Copy it.

![Settings, with a personal Spotify app in use](/assets/images/make-it-even-faster.png)

## Use it in Oxidify

1. Open **Settings**, find **Make it even faster**, and paste the
   Client ID (not the Client Secret). **Show me how** expands the tutorial
   inside Oxidify; **Copy URI** copies the callback used by authorization.
2. Click **Authorize**. Your browser opens Spotify's sign-in for your app.
   Oxidify shows **Authorizing…** while waiting for browser approval and account
   verification. Once it verifies the same Spotify account, the button is
   replaced by **Authorized** and a separate **Remove** control. A failed
   authorization shows an error and makes **Authorize** available again.

### If Spotify reports a redirect mismatch

For `redirect_uri: Not matching configuration`, open the developer dashboard,
select the app whose Client ID you pasted, and edit its **Settings**. Add
`http://127.0.0.1:8989/login` to **Redirect URIs** and **save**. `localhost`,
a different path, HTTPS, or a trailing slash will not match. Return to
Oxidify and click **Authorize** again. Oxidify cannot change your dashboard
configuration automatically.

Authorize with the same Spotify account already signed into Oxidify. If it
is not the app owner, add that account in the dashboard's **User Management**
first, subject to Spotify's Development Mode restrictions.

That is all. Playing music on this computer is unaffected. Select **Remove**
to delete only the personal grant; the shared session stays signed in.
