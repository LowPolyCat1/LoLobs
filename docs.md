
# WebSocket API Documentation

The Rank Overlay Server communicates with the frontend via a WebSocket connection at `ws://127.0.0.1:2009/ws`.

Messages are sent as **JSON strings**. Because the data is sourced directly from the League Client Update (LCU) API, the frontend receives three primary types of data structures depending on the event or state of the application.

## 1. Summoner Data

This message is sent when the LCU first connects or when a user logs in. It provides identity information.

**Key Fields:**

* `gameName`: The user's Riot ID name.
* `tagLine`: The hashtag suffix (e.g., "NA1").
* `summonerLevel`: The current account level.
* `profileIconId`: ID for the user's current avatar.

```json
{
  "gameName": "PlayerOne",
  "tagLine": "EUW",
  "summonerLevel": 432,
  "profileIconId": 1234
}

```

---

## 2. Ranked Stats (Update)

This is the most common message. It is sent whenever the LCU updates the user's ranked information (e.g., after a game finishes) or upon initial connection.

**Key Fields:**

* `queues`: An array of objects containing tier, division, and LP for different game modes.
* `session`: (Added by the Rust Backend) Tracks wins and losses since the **overlay app** was started.

```json
{
  "queues": [
    {
      "queueType": "RANKED_SOLO_5x5",
      "tier": "PLATINUM",
      "division": "II",
      "leaguePoints": 45,
      "wins": 120,
      "losses": 115
    }
  ],
  "session": {
    "wins": 2,
    "losses": 1
  }
}

```

---

## 3. Error / Offline State

If the LCU is not detected or the ranked API returns an error, the backend sends a fallback object to prevent the frontend from crashing.

```json
{
  "queues": [],
  "errorMessage": "LCU_OFFLINE",
  "session": { "wins": 0, "losses": 0 }
}

```

---

## Frontend Implementation Details

The `overlay.js` script handles these messages using a unified logic:

1. **Identity:** If `gameName` exists, it updates the Name and Tag elements.
2. **Ranked:** It searches the `queues` array specifically for `RANKED_SOLO_5x5`.

* It fetches the Tier Emblem from **CommunityDragon** using the `tier` field (e.g., `emblem-platinum.png`).
* It calculates the overall **Win Rate (WR)** using `wins / (wins + losses)`.

1. **Session:** It updates the "Current Session" counters using the custom `session` object injected by the Rust backend.

### Visual States

When a valid message is received, the script adds the `.visible` CSS class to the `.overlay-card` elements to trigger entry animations.
