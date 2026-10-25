# WebSocket API Documentation

The overlay server sends data to the overlay pages through a WebSocket connection at `ws://<ip>:<port>/ws`.
The default address is `ws://127.0.0.1:2009/ws`. To change the address, refer to [Configuration](#configuration).

## Configuration

The server reads the file `config.toml` in the folder of `LoLobs.exe`. When the file does not exist, the server uses the default values.
The file `config.example.toml` shows all keys with their default values.

| Key | Command line flag | Default | Description |
| --- | --- | --- | --- |
| `ip` | `--ip` | `127.0.0.1` | The IP address of the overlay server. |
| `port` | `--port` | `2009` | The port of the overlay server. |
| `queue` | `--queue` | `solo` | The ranked queue: `solo` for Ranked Solo/Duo, `flex` for Ranked Flex. |
| `match_limit` | `--match-limit` | `5` | The maximum number of games in the `matches` message. The value is a number from 1 to 20. |

A command line flag replaces the value from the file, for example `LoLobs.exe --queue flex --match-limit 10`.

When the file or a value is not valid, the server logs an error and stops. The error gives the file, the key and the value.

```toml
ip = "127.0.0.1"
port = 2009
queue = "solo"
match_limit = 5
```

## Message Format

Each message is one JSON text with two fields:

* `type`: The message type. The value is `summoner`, `ranked` or `matches`.
* `data`: The data of the message. The structure of `data` changes with `type`.

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

This message gives the rank of the player in the configured queue.

| Field | Type | Description |
| --- | --- | --- |
| `tier` | string | The tier from the League Client, for example `GOLD` or `MASTER`. |
| `division` | string | The division: `I`, `II`, `III` or `IV`. The field is not present for tiers without divisions, for example `MASTER`. |
| `leaguePoints` | number | The League Points (LP) in the current division. |
| `wins` | number | The wins in this season. |
| `losses` | number | The losses in this season. |
| `session.wins` | number | The wins since the server started. |
| `session.losses` | number | The losses since the server started. |
| `lpChange.lastGame` | number | The LP change of the last game in this session, for example `21` or `-18`. The field is not present before the first game. |
| `lpChange.sessionTotal` | number | The LP change since the server started. |

The server starts the session count and the LP change again when a different account logs in.

The server calculates the LP change on one ladder for all tiers. Each tier from `IRON` to `DIAMOND` has four divisions of 100 LP.
`MASTER`, `GRANDMASTER` and `CHALLENGER` start at the end of `DIAMOND I` and use the LP of the League Client directly.
The server changes `lastGame` only when the sum of wins and losses increases. An update without a new game, for example LP decay, keeps the last value.
When the rank at the start of the session or the current rank has no tier on the ladder, for example during placement games, `lpChange` is not present.

When the player has no data for the configured queue, the server does not send this message.

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
    },
    "lpChange": {
      "lastGame": 21,
      "sessionTotal": 67
    }
  }
}
```

---

## 3. `matches`

This message gives the last games and the current streak of the player in the configured queue.

| Field | Type | Description |
| --- | --- | --- |
| `games` | list | The last games. The list starts with the newest game. The `match_limit` value sets the maximum number of games. |
| `games[].championId` | number | The ID of the champion that the player played. |
| `games[].result` | string | The result of the game: `win`, `loss` or `remake`. The value is `remake` when the game ended with an early surrender. |
| `streak` | object or `null` | The newest games in a row with the same result. The value is `null` when the match history has no win or loss. |
| `streak.kind` | string | The result of the streak: `win` or `loss`. |
| `streak.count` | number | The number of games in the streak. The minimum value is 1. |

The server counts the streak from all games of the configured queue in the match history that the League Client sends.
The `match_limit` value does not change the streak. The server ignores remakes, so a remake does not stop a streak.

```json
{
  "type": "matches",
  "data": {
    "games": [
      { "championId": 103, "result": "win" },
      { "championId": 64, "result": "win" },
      { "championId": 157, "result": "remake" },
      { "championId": 222, "result": "loss" }
    ],
    "streak": { "kind": "win", "count": 2 }
  }
}
```

---

## Overlay Pages

The script `web/overlay.js` reads the `type` field of each message and updates the page:

1. `summoner`: The script shows the Riot ID. When `tagLine` is not present, the script shows no tag.
2. `ranked`: The script shows the tier, the division and the LP. It loads the tier emblem from CommunityDragon.
   The script also calculates the win rate of the season and of the session with `wins / (wins + losses)`.
   `current_rank.html` shows `lpChange.lastGame` next to the LP, green for a gain and red for a loss. When the field is not present, the script hides it.
   `session_winrate.html` shows `lpChange.sessionTotal`, for example `+67 LP`. When `lpChange` is not present, the page shows `-- LP`.
3. `matches`: The script shows one champion icon for each game. The border color of the icon shows the result.
   To show fewer games, add the `matches` parameter to the page URL, for example `past_matches.html?matches=3`.
   The script also shows the streak, for example `3W streak` in green or `2L streak` in red. When `streak` is `null`, the script hides it.

The script ignores messages with an unknown `type`.

When the server sends the page, the script connects to the WebSocket of the same server.
When you open the page as a local file, the script connects to `ws://127.0.0.1:2009/ws`.

### Visual States

When the script receives a known message, it adds the `.visible` CSS class to the `.overlay-card` elements. This class starts the entry animation.

When the rank goes up or down, the script adds a CSS class to `#rank-icon`. This class starts a short animation of the emblem:

* `.rank-promotion`: The tier or the division goes up. The emblem grows and shows a gold glow for 1.2 seconds.
* `.rank-demotion`: The tier or the division goes down. The emblem becomes dark and shakes for 0.8 seconds.

The script removes the class at the end of the animation. A change of LP in the same division does not start an animation. After each connection, the first `ranked` message shows the current rank without an animation.
