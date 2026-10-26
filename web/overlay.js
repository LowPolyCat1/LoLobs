const fallbackSocketUrl = 'ws://127.0.0.1:2009/ws';
const resultClasses = { win: 'win-border', loss: 'loss-border', remake: 'remake' };
const streakLetters = { win: 'W', loss: 'L' };
let socket;

// A page opened as a local file has no server host, so it uses the default backend address.
function socketUrl() {
    const servedOverHttp = location.protocol === 'http:' || location.protocol === 'https:';
    return servedOverHttp ? `ws://${location.host}/ws` : fallbackSocketUrl;
}

function matchLimit() {
    const limit = Number.parseInt(new URLSearchParams(location.search).get('matches'), 10);
    return limit > 0 ? limit : Infinity;
}

function renderStreak(streak) {
    const el = document.getElementById('streak');
    if (!el) return;
    el.classList.remove(...Object.keys(streakLetters));
    el.hidden = !streak || streak.count < 1;
    if (el.hidden) return;
    el.innerText = `${streak.count}${streakLetters[streak.kind]} streak`;
    el.classList.add(streak.kind);
}

function renderMatches(data) {
    renderGames(data.games);
    renderStreak(data.streak);
}

function renderGames(games) {
    const container = document.getElementById('matches-container');
    if (!container) return;

    container.innerHTML = '';
    games.slice(0, matchLimit()).forEach(game => {
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
    if (tagEl) tagEl.innerText = data.tagLine ? `#${data.tagLine}` : '';
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

// Zero has no sign and keeps the default color.
function setSignedValue(el, value, suffix) {
    el.innerText = `${value > 0 ? '+' : ''}${value}${suffix}`;
    el.classList.toggle('win', value > 0);
    el.classList.toggle('loss', value < 0);
}

function renderLastGameLp(lpChange) {
    const el = document.getElementById('lp-change');
    if (!el) return;
    const lastGame = lpChange ? lpChange.lastGame : undefined;
    el.hidden = lastGame === undefined;
    if (lastGame !== undefined) setSignedValue(el, lastGame, '');
}

function renderSessionLp(lpChange) {
    const el = document.getElementById('s-lp');
    if (!el) return;
    if (lpChange) {
        setSignedValue(el, lpChange.sessionTotal, ' LP');
        return;
    }
    el.innerText = '-- LP';
    el.classList.remove('win', 'loss');
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
    renderLastGameLp(data.lpChange);
    renderSessionLp(data.lpChange);
}

// The phase card has its own visibility: a phase message with null data hides it.
function renderPhase(data) {
    const card = document.getElementById('phase-card');
    if (!card) return;
    card.classList.toggle('visible', Boolean(data));
    if (!data) return;
    document.getElementById('phase-label').innerText = data.label;
    const queueEl = document.getElementById('phase-queue');
    queueEl.hidden = !data.queue;
    queueEl.innerText = data.queue || '';
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
        case 'phase':
            renderPhase(msg.data);
            return;
        default:
            console.warn("Unknown overlay message type:", msg.type);
            return;
    }

    // Reveal cards once data arrives
    document.querySelectorAll('.overlay-card:not(#phase-card)').forEach(el => el.classList.add('visible'));
}

function connect() {
    previousRank = null;
    socket = new WebSocket(socketUrl());

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
