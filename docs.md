# WebSocket API Documentation

The overlay server sends data to the overlay pages through a WebSocket connection at `ws://127.0.0.1:2009/ws`.

## Message Format

Each message is one JSON text with two fields:

* `type`: The message type. The value is `summoner`, `ranked` or `matches`.
* `data`: The data of the message. The structure of `data` changes with `type`. For `matches`, `data` is a list.

```json
{ "type": "summoner", "data": { "gameName": "PlayerOne", "tagLine": "EUW" } }
```

The server reads the data from the League Client (LCU API). It sends only the fields that the overlay uses.

## When the Server Sends Messages

* When the League Client connects, the server sends one message of each type.
* When the League Client changes the summoner, ranked stats or match history, the server sends a new message of that type.
* When the ranked stats change, the server also sends a new `matches` message.
* When a page connects, the server sends the last message of each type that it has.
* When a page cannot read the messages fast enough, the server sends the last message of each type again.

A new message of a type replaces all data of the last message of that type.

---

## 1. `summoner`

This message gives the Riot ID of the player.

| Field | Type | Description |
| --- | --- | --- |
| `gameName` | string | The name part of the Riot ID. |
| `tagLine` | string | The tag part of the Riot ID, without `#`. The field is not present when the League Client does not send a tag. |

```json
{
  "type": "summoner",
  "data": {
    "gameName": "PlayerOne",
    "tagLine": "EUW"
  }
}
```

---

## 2. `ranked`

This message gives the rank of the player in the Ranked Solo/Duo queue.

| Field | Type | Description |
| --- | --- | --- |
| `tier` | string | The tier from the League Client, for example `GOLD` or `MASTER`. |
| `division` | string | The division: `I`, `II`, `III` or `IV`. The field is not present for tiers without divisions, for example `MASTER`. |
| `leaguePoints` | number | The League Points (LP) in the current division. |
| `wins` | number | The wins in this season. |
| `losses` | number | The losses in this season. |
| `session.wins` | number | The wins since the server started. |
| `session.losses` | number | The losses since the server started. |

The server starts the session count again when a different account logs in.

When the player has no Ranked Solo/Duo data, the server does not send this message.

```json
{
  "type": "ranked",
  "data": {
    "tier": "GOLD",
    "division": "II",
    "leaguePoints": 57,
    "wins": 34,
    "losses": 29,
    "session": {
      "wins": 2,
      "losses": 1
    }
  }
}
```

---

## 3. `matches`

This message gives the last Ranked Solo/Duo games of the player. The `data` field is a list. The list starts with the newest game and has a maximum of 5 games.

| Field | Type | Description |
| --- | --- | --- |
| `championId` | number | The ID of the champion that the player played. |
| `result` | string | The result of the game: `win`, `loss` or `remake`. |

```json
{
  "type": "matches",
  "data": [
    { "championId": 103, "result": "win" },
    { "championId": 222, "result": "loss" },
    { "championId": 157, "result": "remake" }
  ]
}
```

---

## Overlay Pages

The script `web/overlay.js` reads the `type` field of each message and updates the page:

1. `summoner`: The script shows the Riot ID. When `tagLine` is not present, the script shows `#NA1`.
2. `ranked`: The script shows the tier, the division and the LP. It loads the tier emblem from CommunityDragon.
   The script also calculates the win rate of the season and of the session with `wins / (wins + losses)`.
3. `matches`: The script shows one champion icon for each game. The border color of the icon shows the result.

The script ignores messages with an unknown `type`.

### Visual States

When the script receives a known message, it adds the `.visible` CSS class to the `.overlay-card` elements. This class starts the entry animation.
