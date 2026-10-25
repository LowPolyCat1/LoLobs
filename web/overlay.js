const socketUrl = 'ws://127.0.0.1:2009/ws';
const resultClasses = { win: 'win-border', loss: 'loss-border', remake: 'remake' };
let socket;

function renderMatches(games) {
    const container = document.getElementById('matches-container');
    if (!container) return;

    container.innerHTML = '';
    games.forEach(game => {
        const img = document.createElement('img');
        // Using CommunityDragon for champion icons
        img.src = `https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/v1/champion-icons/${game.championId}.png`;
        img.className = `match-icon ${resultClasses[game.result]}`;
        container.appendChild(img);
    });
}

function renderSummoner(data) {
    const nameEl = document.getElementById('game-name');
    const tagEl = document.getElementById('tag-line');
    if (nameEl) nameEl.innerText = data.gameName;
    if (tagEl) tagEl.innerText = `#${data.tagLine || 'NA1'}`;
}

function renderRank(data) {
    const tierEl = document.getElementById('tier');
    const lpEl = document.getElementById('lp');
    if (tierEl) tierEl.innerText = `${data.tier} ${data.division || ''}`.trim();
    if (lpEl) lpEl.innerText = `${data.leaguePoints} LP`;

    const iconImg = document.getElementById('rank-icon');
    if (iconImg && data.tier) {
        const newSrc = `https://raw.communitydragon.org/latest/plugins/rcp-fe-lol-static-assets/global/default/images/ranked-emblem/emblem-${data.tier.toLowerCase()}.png`;
        if (iconImg.src !== newSrc) iconImg.src = newSrc;
    }
}

function renderSeason(data) {
    if (!document.getElementById('wins')) return;

    const total = data.wins + data.losses;
    const wr = total > 0 ? ((data.wins / total) * 100).toFixed(1) : "0.0";
    document.getElementById('wins').innerText = data.wins;
    document.getElementById('losses').innerText = data.losses;
    document.getElementById('wr-value').innerText = `${wr}%`;
}

function renderSession(session) {
    const sTotal = session.wins + session.losses;
    const sWr = sTotal > 0 ? ((session.wins / sTotal) * 100).toFixed(0) : "0";

    if (document.getElementById('s-wins')) document.getElementById('s-wins').innerText = session.wins;
    if (document.getElementById('s-losses')) document.getElementById('s-losses').innerText = session.losses;
    if (document.getElementById('s-wr')) document.getElementById('s-wr').innerText = `${sWr}%`;
}

const TIER_ORDER = ['IRON', 'BRONZE', 'SILVER', 'GOLD', 'PLATINUM', 'EMERALD', 'DIAMOND', 'MASTER', 'GRANDMASTER', 'CHALLENGER'];
const DIVISION_ORDER = ['IV', 'III', 'II', 'I'];
const RANK_CHANGE_CLASSES = { promotion: 'rank-promotion', demotion: 'rank-demotion' };
// Null after each (re)connect, so that the initial state does not animate.
let previousRank = null;

// Apex tiers have no division, so they count as the lowest division of their tier.
function rankValue(rank) {
    const tierIndex = TIER_ORDER.indexOf(rank.tier);
    if (tierIndex < 0) return null;
    const divisionIndex = Math.max(DIVISION_ORDER.indexOf(rank.division), 0);
    return tierIndex * DIVISION_ORDER.length + divisionIndex;
}

function rankChange(previous, next) {
    if (!previous) return 'none';
    const before = rankValue(previous);
    const after = rankValue(next);
    if (before === null || after === null || before === after) return 'none';
    return after > before ? 'promotion' : 'demotion';
}

function clearRankAnimation(event) {
    event.target.classList.remove(...Object.values(RANK_CHANGE_CLASSES));
}

function playRankAnimation(change) {
    const icon = document.getElementById('rank-icon');
    const className = RANK_CHANGE_CLASSES[change];
    if (!icon || !className) return;
    icon.classList.remove(...Object.values(RANK_CHANGE_CLASSES));
    // Forces a reflow, so that the same class restarts its animation.
    void icon.offsetWidth;
    icon.addEventListener('animationend', clearRankAnimation);
    icon.classList.add(className);
}

function animateRankChange(data) {
    const change = rankChange(previousRank, data);
    previousRank = { tier: data.tier, division: data.division };
    playRankAnimation(change);
}

function renderRanked(data) {
    renderRank(data);
    animateRankChange(data);
    renderSeason(data);
    renderSession(data.session);
}

function handleMessage(msg) {
    switch (msg.type) {
        case 'summoner':
            renderSummoner(msg.data);
            break;
        case 'ranked':
            renderRanked(msg.data);
            break;
        case 'matches':
            renderMatches(msg.data);
            break;
        default:
            console.warn("Unknown overlay message type:", msg.type);
            return;
    }

    // Reveal cards once data arrives
    document.querySelectorAll('.overlay-card').forEach(el => el.classList.add('visible'));
}

function connect() {
    previousRank = null;
    socket = new WebSocket(socketUrl);

    socket.onopen = () => {
        console.log("%c CONNECTED TO RUST BACKEND ", "background: #222; color: #bada55");
    };

    socket.onmessage = (event) => {
        try {
            handleMessage(JSON.parse(event.data));
        } catch (e) {
            console.error("Overlay update error:", e);
        }
    };

    socket.onclose = () => {
        setTimeout(connect, 2000);
    };

    socket.onerror = (err) => {
        socket.close();
    };
}

connect();
